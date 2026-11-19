//! Scene lessons' pictures: one illustrated scene — a market stall, a
//! kitchen, a waiting room — holding the lesson's words as things to find.
//!
//! Made in the background in two steps, like the course and word pictures:
//! an image model draws the scene from the tutor's description and the
//! lesson's words (in the same style as every other picture in the app),
//! then a vision model is shown the picture and asked where each thing is,
//! as a box in fractions of the picture (0–1). Boxes that make no sense are
//! dropped; a thing it cannot find is dropped from the lesson; too few
//! found and it looks again, then draws again, then gives up with a reason
//! and a button to try again.
//!
//! Only the device that started a scene works on it (`sync::written_here`):
//! another device sees "being drawn" until the finished scene syncs over.

use std::future::Future;

use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::courses::NewWord;
use crate::db::{self, Conn, Db};
use crate::error::{Error, Result};
use crate::files;
#[allow(unused_imports)]
use crate::ids::*;
use crate::lessons::Lesson;
use crate::schema::{scene_spots, scenes, words};

/// Things in a scene: enough to explore and choose between, few enough to
/// draw clearly and tap on a phone.
pub const MIN_THINGS: usize = 6;
pub const MAX_THINGS: usize = 10;
/// Fewer found than this and the scene is looked at again, or drawn again.
pub const MIN_SPOTS: usize = 5;
/// "Where is it?" sentences a scene keeps.
pub const MAX_STATEMENTS: usize = 6;
/// The prepositions whose sentences can be checked against the boxes (see
/// [`holds`]): a sentence the picture does not bear out is never asked.
pub const PREPOSITIONS: &[&str] = &["pod", "na", "nad", "obok", "przy", "w"];
/// The longest side a scene is kept at: sharp on a large screen, a few
/// hundred kilobytes in the database.
const SCENE_SIDE: u32 = 1280;
/// Wider than tall, so the picture and the answers fit a phone together.
const ASPECT: &str = "4:3";
/// Drawings tried, and looks at each, before giving up.
const DRAWINGS: usize = 2;
const LOOKS: usize = 2;

pub const DRAWING: &str = "drawing";
pub const LOCATING: &str = "locating";
pub const READY: &str = "ready";
pub const FAILED: &str = "failed";

// ---------------------------------------------------------------------------
// Boxes
// ---------------------------------------------------------------------------

/// A box in a picture, in fractions of its width and height from the top
/// left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Area {
    fn centre(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    fn area(&self) -> f64 {
        self.w * self.h
    }

    /// How much two boxes are one: their overlap over their union.
    fn iou(&self, other: &Area) -> f64 {
        let w = (self.x + self.w).min(other.x + other.w) - self.x.max(other.x);
        let h = (self.y + self.h).min(other.y + other.h) - self.y.max(other.y);
        if w <= 0.0 || h <= 0.0 {
            return 0.0;
        }
        let both = w * h;
        both / (self.area() + other.area() - both)
    }

    /// How much of the narrower box's width the two share.
    fn shared_width(&self, other: &Area) -> f64 {
        let w = (self.x + self.w).min(other.x + other.w) - self.x.max(other.x);
        (w / self.w.min(other.w)).max(0.0)
    }
}

/// Smallest side a found thing may have: anything smaller is a speck no one
/// could tap, and more likely a guess.
const MIN_SIDE: f64 = 0.02;
/// Largest share of the picture one thing may cover: more is the model
/// boxing the whole scene.
const MAX_AREA: f64 = 0.6;
/// Two things boxed (nearly) the same: the model could not tell them apart.
const SAME_BOX: f64 = 0.85;

/// Whether a sentence's preposition is borne out by where its two things
/// are: the subject under, on, above, beside or in the object. Lenient
/// where a drawing can be (a cat "pod stołem" may stick out a little), strict
/// where it matters (under is below, not beside).
pub fn holds(preposition: &str, subject: &Area, object: &Area) -> bool {
    let (sx, sy) = subject.centre();
    let (ox, oy) = object.centre();
    let shared = subject.shared_width(object);
    match preposition {
        "pod" => sy > oy && shared > 0.3,
        "na" => sy < oy && shared > 0.3 && subject.y < object.y + object.h / 2.0,
        "nad" => sy < object.y + object.h * 0.2 && shared > 0.0,
        // Side by side: further apart across than up and down, and not one
        // inside the other.
        "obok" | "przy" => (sx - ox).abs() > (sy - oy).abs() * 0.8 && subject.iou(object) < 0.5,
        "w" => sx > object.x && sx < object.x + object.w && sy > object.y && sy < object.y + object.h,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// The vision model's reply
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct Located {
    things: Vec<LocatedThing>,
}

#[derive(Deserialize)]
struct LocatedThing {
    n: i64,
    #[serde(default = "yes")]
    found: bool,
    #[serde(default)]
    x_min: Option<f64>,
    #[serde(default)]
    y_min: Option<f64>,
    #[serde(default)]
    x_max: Option<f64>,
    #[serde(default)]
    y_max: Option<f64>,
}

fn yes() -> bool {
    true
}

/// The JSON Schema the vision model answers in (strict structured output).
pub fn spots_schema() -> serde_json::Value {
    let number = serde_json::json!({ "type": "number" });
    serde_json::json!({
        "type": "object",
        "properties": {
            "things": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "n": { "type": "integer" },
                        "found": { "type": "boolean" },
                        "x_min": number, "y_min": number, "x_max": number, "y_max": number
                    },
                    "required": ["n", "found", "x_min", "y_min", "x_max", "y_max"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["things"],
        "additionalProperties": false
    })
}

/// Reads where the things are from a vision model's reply: thing `n`
/// (numbered from 1, of `count`) and its box. Things not found, or boxed
/// out of the picture, too small, too large, or on top of another, are
/// left out; a reply whose boxes are all the same (the model did not look)
/// is an error. Boxes in thousandths (Gemini's own habit) or in pixels of a
/// `width` × `height` picture are read as fractions.
pub fn parse_spots(text: &str, count: usize, width: u32, height: u32) -> std::result::Result<Vec<(usize, Area)>, String> {
    let located: Located = crate::ai::json_in(text)?;
    let mut raw: Vec<(usize, [f64; 4])> = Vec::new();
    for thing in located.things {
        let Ok(n) = usize::try_from(thing.n) else { continue };
        if n == 0 || n > count || raw.iter().any(|(m, _)| *m == n) || !thing.found {
            continue;
        }
        let (Some(x0), Some(y0), Some(x1), Some(y1)) = (thing.x_min, thing.y_min, thing.x_max, thing.y_max) else {
            continue;
        };
        if [x0, y0, x1, y1].iter().all(|v| v.is_finite()) {
            raw.push((n, [x0, y0, x1, y1]));
        }
    }
    // The scale: fractions, thousandths or pixels, the same for every box.
    let largest = raw.iter().flat_map(|(_, b)| b.iter().copied()).fold(0.0_f64, f64::max);
    let (sx, sy) = if largest <= 1.5 {
        (1.0, 1.0)
    } else if largest <= 1000.5 {
        (1000.0, 1000.0)
    } else {
        (f64::from(width.max(1)), f64::from(height.max(1)))
    };
    let mut found: Vec<(usize, Area)> = Vec::new();
    for (n, [x0, y0, x1, y1]) in raw {
        let (x0, x1) = (x0.min(x1) / sx, x0.max(x1) / sx);
        let (y0, y1) = (y0.min(y1) / sy, y0.max(y1) / sy);
        // A little over the edge is a loose box; far over is a wrong one.
        if x0 < -0.05 || y0 < -0.05 || x1 > 1.05 || y1 > 1.05 {
            continue;
        }
        let (x0, y0, x1, y1) = (x0.max(0.0), y0.max(0.0), x1.min(1.0), y1.min(1.0));
        let area = Area { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
        if area.w < MIN_SIDE || area.h < MIN_SIDE || area.area() > MAX_AREA {
            continue;
        }
        found.push((n, area));
    }
    if found.len() >= 3 && found.iter().all(|(_, a)| a.iou(&found[0].1) > 0.9) {
        return Err("every thing was given the same box".into());
    }
    // Two things in one box: neither can be found by tapping it.
    let doubled: Vec<usize> = found
        .iter()
        .filter(|(n, a)| found.iter().any(|(m, b)| m != n && a.iou(b) > SAME_BOX))
        .map(|(n, _)| *n)
        .collect();
    found.retain(|(n, _)| !doubled.contains(n));
    Ok(found)
}

// ---------------------------------------------------------------------------
// Prompts
// ---------------------------------------------------------------------------

/// A thing in the scene: one of the lesson's words.
#[derive(Debug, Clone)]
pub struct Thing {
    pub word_id: WordId,
    pub pl: String,
    pub en: String,
}

/// A gloss's first meaning: "glass, tumbler" → "glass".
fn first_meaning(en: &str) -> &str {
    en.split([',', ';', '(']).next().unwrap_or(en).trim()
}

/// What the image model is asked to draw. Made from the description, the
/// things and the arrangement the sentences need, and the app's one picture
/// style — so drawing it again asks for the same thing.
pub fn image_prompt(description: &str, things: &[Thing], arrangement: &[String]) -> String {
    let list = things.iter().map(|t| first_meaning(&t.en).to_string()).collect::<Vec<_>>().join(", ");
    let mut prompt = format!(
        "Draw one illustrated scene for a Polish vocabulary lesson: {}. These things must be in it, \
         each one clearly visible, whole, unmistakable and apart from the others — not overlapping, \
         not hidden behind anything, not cut off at the edges — and each only once: {list}.",
        description.trim().trim_end_matches('.')
    );
    if !arrangement.is_empty() {
        prompt.push_str(&format!(" Arrange it so that: {}", arrangement.join(" ")));
    }
    prompt.push_str(
        " Style: a flat, colourful vector illustration with simple shapes and soft light, like a \
         picture book: one scene seen from the front, every one of those things large enough to \
         point at, a calm background with few other objects. A hint of Poland where it fits. \
         Landscape, 4:3. No text, letters, words or numbers anywhere in the image.",
    );
    prompt
}

/// What the vision model is asked: where each numbered thing is.
pub fn locate_prompt(things: &[Thing]) -> String {
    let list = things
        .iter()
        .enumerate()
        .map(|(i, t)| format!("{}. {} (Polish: {})", i + 1, first_meaning(&t.en), t.pl))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "This is an illustration of a scene. Find each of these things in it:\n\n{list}\n\n\
         For every number give: found — true if the thing is clearly in the picture, false if it is \
         not there or you cannot tell which it is; and its box — x_min, y_min, x_max, y_max as \
         fractions of the picture's width and height, from 0 to 1, where (0, 0) is the top-left \
         corner and (1, 1) the bottom-right. The box fits tightly around the whole thing and \
         nothing else. If a thing appears more than once, box the clearest one. When not found, \
         give 0 for each coordinate. Answer with JSON only: \
         {{\"things\": [{{\"n\": 1, \"found\": true, \"x_min\": 0.12, \"y_min\": 0.30, \"x_max\": 0.25, \"y_max\": 0.48}}, …]}}, \
         one entry for every number."
    )
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// A "Where is it?" sentence, as stored: the true sentence, the two things
/// it is about, and the phrase in it that says where — with wrong phrases
/// for a false sentence or a choice.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Statement {
    pub pl: String,
    pub en: String,
    pub subject: WordId,
    pub object: WordId,
    pub preposition: String,
    /// The words of `pl` that say where: "pod stołem".
    pub phrase: String,
    /// The same place said wrongly for this picture: "na stole".
    pub wrong: Vec<String>,
}

/// One thing in a scene, for the screens.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SceneSpot {
    pub word_id: WordId,
    pub pl: String,
    pub en: String,
    /// Its box, in fractions of the picture (0–1, from the top left).
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// The card points it out (a soft ring): the thing to name, or the two
    /// things a sentence is about.
    pub lit: bool,
}

/// A scene as a card shows it: the picture, its size, and its things.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SceneCard {
    pub image: FileId,
    pub width: i64,
    pub height: i64,
    pub spots: Vec<SceneSpot>,
}

/// A scene lesson's picture as its page shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneInfo {
    /// drawing | locating | ready | failed
    pub status: String,
    /// Why it failed, when it did.
    pub error: String,
    /// Being made on this device right now (another device's scene in the
    /// making shows its status, but only that device carries on).
    pub working: bool,
    pub description: String,
    pub card: Option<SceneCard>,
    /// The "Where is it?" sentences the picture bears out: (Polish, English).
    pub statements: Vec<(String, String)>,
}

/// What a scene row holds.
#[derive(Queryable, Selectable)]
#[diesel(table_name = scenes, check_for_backend(diesel::sqlite::Sqlite))]
struct SceneRow {
    description: String,
    statements: String,
    image_file_id: Option<FileId>,
    width: i64,
    height: i64,
    status: String,
    error: String,
}

/// Starts a lesson's scene: nothing drawn yet.
pub fn insert(conn: &mut Conn, lesson_id: LessonId, description: &str, statements: &[Statement]) -> Result<()> {
    let now = db::now_secs();
    diesel::insert_into(scenes::table)
        .values((
            scenes::lesson_id.eq(lesson_id),
            scenes::description.eq(description.trim()),
            scenes::statements.eq(serde_json::to_string(statements)?),
            scenes::status.eq(DRAWING),
            scenes::created_at.eq(now),
            scenes::updated_at.eq(now),
        ))
        .execute(conn)?;
    Ok(())
}

/// The lesson's words, the things its scene shows, in order.
pub fn things(conn: &mut Conn, lesson_id: LessonId) -> Result<Vec<Thing>> {
    Ok(words::table
        .filter(words::lesson_id.eq(lesson_id))
        .order((words::ord, words::id))
        .select((words::id, words::pl, words::en))
        .load::<(WordId, String, String)>(conn)?
        .into_iter()
        .map(|(word_id, pl, en)| Thing { word_id, pl, en })
        .collect())
}

fn spots(conn: &mut Conn, lesson_id: LessonId) -> Result<Vec<SceneSpot>> {
    Ok(scene_spots::table
        .inner_join(words::table)
        .filter(scene_spots::lesson_id.eq(lesson_id))
        .order((words::ord, words::id))
        .select((words::id, words::pl, words::en, scene_spots::x, scene_spots::y, scene_spots::w, scene_spots::h))
        .load::<(WordId, String, String, f64, f64, f64, f64)>(conn)?
        .into_iter()
        .map(|(word_id, pl, en, x, y, w, h)| SceneSpot { word_id, pl, en, x, y, w, h, lit: false })
        .collect())
}

fn area(spot: &SceneSpot) -> Area {
    Area { x: spot.x, y: spot.y, w: spot.w, h: spot.h }
}

/// The sentences the picture bears out: both things found, and where they
/// are matches what the sentence says.
pub fn checked<'a>(statements: &'a [Statement], spots: &[SceneSpot]) -> Vec<&'a Statement> {
    let find = |id: WordId| spots.iter().find(|s| s.word_id == id).map(area);
    statements
        .iter()
        .filter(|s| match (find(s.subject), find(s.object)) {
            (Some(subject), Some(object)) => holds(&s.preposition, &subject, &object),
            _ => false,
        })
        .collect()
}

/// A finished scene: its picture and things, and its checked sentences.
/// None while it is being made, or when it failed.
pub fn card(conn: &mut Conn, lesson_id: LessonId) -> Result<Option<(SceneCard, Vec<Statement>)>> {
    let Some(row) = scenes::table.find(lesson_id).select(SceneRow::as_select()).first(conn).optional()? else {
        return Ok(None);
    };
    let (Some(image), READY) = (row.image_file_id, row.status.as_str()) else {
        return Ok(None);
    };
    let spots = spots(conn, lesson_id)?;
    if spots.is_empty() {
        return Ok(None);
    }
    let stored: Vec<Statement> = serde_json::from_str(&row.statements).unwrap_or_default();
    let statements = checked(&stored, &spots).into_iter().cloned().collect();
    Ok(Some((SceneCard { image, width: row.width, height: row.height, spots }, statements)))
}

/// A lesson's scene for its page; None for a lesson without one.
pub fn info(conn: &mut Conn, lesson_id: LessonId) -> Result<Option<SceneInfo>> {
    let Some(row) = scenes::table.find(lesson_id).select(SceneRow::as_select()).first(conn).optional()? else {
        return Ok(None);
    };
    let (card, statements) = match card(conn, lesson_id)? {
        Some((card, statements)) => (Some(card), statements.into_iter().map(|s| (s.pl, s.en)).collect()),
        None => (None, Vec::new()),
    };
    Ok(Some(SceneInfo {
        working: is_making(lesson_id),
        status: row.status,
        error: row.error,
        description: row.description,
        card,
        statements,
    }))
}

fn set_status(conn: &mut Conn, lesson_id: LessonId, status: &str, error: &str) -> Result<()> {
    diesel::update(scenes::table.find(lesson_id))
        .set((scenes::status.eq(status), scenes::error.eq(error), scenes::updated_at.eq(db::now_secs())))
        .execute(conn)?;
    Ok(())
}

/// Keeps a newly drawn picture as the scene's, in place of the old one and
/// its things' boxes.
fn store_picture(conn: &mut Conn, lesson_id: LessonId, model: &str, prompt: &str, jpeg: &[u8], width: u32, height: u32) -> Result<FileId> {
    conn.transaction(|conn| {
        let old: Option<FileId> = scenes::table.find(lesson_id).select(scenes::image_file_id).first(conn)?;
        let name = format!("scene-{lesson_id}-{}.jpg", db::now_secs());
        let id = files::store(conn, files::SCENE_IMAGE, &name, "image/jpeg", jpeg)?;
        diesel::update(scenes::table.find(lesson_id))
            .set((
                scenes::image_file_id.eq(id),
                scenes::width.eq(i64::from(width)),
                scenes::height.eq(i64::from(height)),
                scenes::prompt.eq(prompt),
                scenes::image_model.eq(model),
                scenes::status.eq(LOCATING),
                scenes::updated_at.eq(db::now_secs()),
            ))
            .execute(conn)?;
        diesel::delete(scene_spots::table.filter(scene_spots::lesson_id.eq(lesson_id))).execute(conn)?;
        if let Some(old) = old {
            files::delete(conn, &[old])?;
        }
        Ok(id)
    })
}

/// Keeps where the things are, and finishes the scene. A thing the model
/// could not find leaves the lesson: it is not in the picture to practise.
fn store_spots(conn: &mut Conn, lesson_id: LessonId, model: &str, things: &[Thing], found: &[(usize, Area)]) -> Result<()> {
    conn.transaction(|conn| {
        diesel::delete(scene_spots::table.filter(scene_spots::lesson_id.eq(lesson_id))).execute(conn)?;
        for (n, a) in found {
            let Some(thing) = things.get(n - 1) else { continue };
            diesel::insert_into(scene_spots::table)
                .values((
                    scene_spots::id.eq(SpotId::new()),
                    scene_spots::lesson_id.eq(lesson_id),
                    scene_spots::word_id.eq(thing.word_id),
                    scene_spots::x.eq(a.x),
                    scene_spots::y.eq(a.y),
                    scene_spots::w.eq(a.w),
                    scene_spots::h.eq(a.h),
                ))
                .execute(conn)?;
        }
        let missing: Vec<WordId> = things
            .iter()
            .enumerate()
            .filter(|(i, _)| !found.iter().any(|(n, _)| *n == i + 1))
            .map(|(_, t)| t.word_id)
            .collect();
        diesel::delete(words::table.filter(words::id.eq_any(&missing))).execute(conn)?;
        diesel::update(scenes::table.find(lesson_id))
            .set((
                scenes::vision_model.eq(model),
                scenes::status.eq(READY),
                scenes::error.eq(""),
                scenes::updated_at.eq(db::now_secs()),
            ))
            .execute(conn)?;
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Making a scene lesson
// ---------------------------------------------------------------------------

/// A "Where is it?" sentence as the tutor writes it: its two things by
/// their Polish.
#[derive(Debug, Clone, Deserialize)]
pub struct NewStatement {
    pub pl: String,
    pub en: String,
    pub subject: String,
    pub preposition: String,
    pub object: String,
    pub phrase: String,
    #[serde(default)]
    pub wrong: Vec<String>,
}

/// A scene lesson, as the tutor asks for one.
#[derive(Debug, Clone, Deserialize)]
pub struct NewScene {
    pub title: String,
    #[serde(default)]
    pub summary: String,
    /// The scene, in English: the place and what is going on in it.
    pub scene: String,
    #[serde(default)]
    pub complexity: Option<String>,
    pub things: Vec<NewWord>,
    #[serde(default)]
    pub statements: Vec<NewStatement>,
    #[serde(default, alias = "topic_id")]
    pub course_id: Option<CourseId>,
    #[serde(default)]
    pub period_days: Option<i64>,
}

fn same(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// Checks a scene lesson before anything is written, so a mistake comes
/// back to the tutor whole: the things to fix, not a half-made lesson.
fn check(input: &NewScene) -> Result<()> {
    if input.scene.trim().is_empty() {
        return Err(Error::msg("Describe the scene: the place and what is going on in it."));
    }
    let n = input.things.len();
    if !(MIN_THINGS..=MAX_THINGS).contains(&n) {
        return Err(Error::msg(format!("A scene needs {MIN_THINGS}–{MAX_THINGS} things (got {n}).")));
    }
    for (i, thing) in input.things.iter().enumerate() {
        if thing.pl.trim().is_empty() || thing.en.trim().is_empty() {
            return Err(Error::msg("Every thing needs its Polish and its English."));
        }
        if !thing.pos.starts_with("n-") {
            return Err(Error::msg(format!(
                "“{}” is not a thing you can point at: a scene's things are nouns (n-m, n-f, n-n, n-pl).",
                thing.pl
            )));
        }
        if input.things[..i].iter().any(|other| same(&other.pl, &thing.pl)) {
            return Err(Error::msg(format!("“{}” is in the scene twice.", thing.pl)));
        }
    }
    if input.statements.len() > MAX_STATEMENTS {
        return Err(Error::msg(format!("At most {MAX_STATEMENTS} “Where is it?” sentences.")));
    }
    for s in &input.statements {
        let named = |pl: &str| input.things.iter().any(|t| same(&t.pl, pl));
        if !named(&s.subject) || !named(&s.object) || same(&s.subject, &s.object) {
            return Err(Error::msg(format!(
                "“{}”: its subject and object must be two of the scene's things, by their Polish as listed.",
                s.pl
            )));
        }
        if !PREPOSITIONS.contains(&s.preposition.as_str()) {
            return Err(Error::msg(format!(
                "“{}”: use one of {} — prepositions the app can check against the picture.",
                s.pl,
                PREPOSITIONS.join(", ")
            )));
        }
        if s.phrase.trim().is_empty() || !s.pl.contains(s.phrase.trim()) {
            return Err(Error::msg(format!("“{}”: its phrase must be written exactly as it is in the sentence.", s.pl)));
        }
        let wrong: Vec<&String> = s.wrong.iter().filter(|w| !w.trim().is_empty()).collect();
        if wrong.is_empty() || wrong.len() > 3 || wrong.iter().any(|w| same(w, &s.phrase)) {
            return Err(Error::msg(format!(
                "“{}”: give 1–3 wrong phrases, each a different place for the same thing (\"na stole\" for \"pod stołem\").",
                s.pl
            )));
        }
    }
    Ok(())
}

/// Makes a scene lesson: its things become the lesson's words (practised
/// and reviewed like any others), and its scene is waiting to be drawn —
/// [`resume_pending`] draws it in the background.
pub fn create(conn: &mut Conn, input: NewScene, origin: &str) -> Result<Lesson> {
    check(&input)?;
    let complexity = input.complexity.as_deref().unwrap_or("low");
    let course_id = match input.course_id {
        Some(id) => id,
        None => crate::courses::other_course(conn)?,
    };
    let items = crate::courses::Items { words: input.things.clone(), sentences: vec![], dialogue: vec![] };
    let (lesson, _) = crate::lessons::create_drill(
        conn,
        course_id,
        &input.title,
        &input.summary,
        "scene",
        complexity,
        &items,
        input.period_days,
        origin,
    )?;
    let things = things(conn, lesson.id)?;
    let id_of = |pl: &str| things.iter().find(|t| same(&t.pl, pl)).map(|t| t.word_id);
    let statements: Vec<Statement> = input
        .statements
        .iter()
        .filter_map(|s| {
            Some(Statement {
                pl: crate::text::clean(&s.pl),
                en: crate::text::clean(&s.en),
                subject: id_of(&s.subject)?,
                object: id_of(&s.object)?,
                preposition: s.preposition.clone(),
                phrase: s.phrase.trim().to_string(),
                wrong: s.wrong.iter().map(|w| w.trim().to_string()).filter(|w| !w.is_empty()).collect(),
            })
        })
        .collect();
    insert(conn, lesson.id, &input.scene, &statements)?;
    Ok(lesson)
}

/// The models a scene is made with: the real ones on OpenRouter, or
/// stand-ins in tests.
pub trait Models: Sync {
    /// Draws a picture: its type, bytes and cost.
    fn draw(&self, prompt: &str) -> impl Future<Output = Result<(String, Vec<u8>, Option<f64>)>> + Send;
    /// Asks where the things are in a picture.
    fn look(&self, prompt: &str, mime: &str, image: &[u8]) -> impl Future<Output = Result<crate::ai::Reply>> + Send;
    fn image_model(&self) -> &str;
    fn vision_model(&self) -> &str;
}

/// The learner's chosen models on OpenRouter.
pub struct OpenRouter {
    key: String,
    image_model: String,
    vision_model: String,
}

impl OpenRouter {
    pub fn chosen(conn: &mut Conn) -> Result<Self> {
        Ok(Self { key: crate::ai::api_key(conn)?, image_model: crate::ai::image_model(conn), vision_model: crate::ai::vision_model(conn) })
    }
}

impl Models for OpenRouter {
    fn draw(&self, prompt: &str) -> impl Future<Output = Result<(String, Vec<u8>, Option<f64>)>> + Send {
        crate::ai::generate_image_shaped(&self.key, &self.image_model, prompt, Some(ASPECT))
    }

    fn look(&self, prompt: &str, mime: &str, image: &[u8]) -> impl Future<Output = Result<crate::ai::Reply>> + Send {
        let schema = spots_schema();
        async move { crate::ai::look_at_image(&self.key, &self.vision_model, prompt, mime, image, "scene_spots", &schema).await }
    }

    fn image_model(&self) -> &str {
        &self.image_model
    }

    fn vision_model(&self) -> &str {
        &self.vision_model
    }
}

/// Makes (or remakes) a lesson's scene: draws it unless it has a picture
/// and `redraw` is false, then finds its things — looking again, then
/// drawing again, when too few are found. Returns how many were found.
/// Every step is written as it happens, so the page can say where it is;
/// a failure is written too, with its reason.
pub async fn make(db: &Db, lesson_id: LessonId, redraw: bool, models: &impl Models, on_status: &(dyn Fn(&str) + Sync)) -> Result<usize> {
    let outcome = attempt(db, lesson_id, redraw, models, on_status).await;
    if let Err(error) = &outcome {
        let _ = set_status(&mut db.lock(), lesson_id, FAILED, &error.to_string());
        on_status(FAILED);
    }
    outcome
}

async fn attempt(db: &Db, lesson_id: LessonId, redraw: bool, models: &impl Models, on_status: &(dyn Fn(&str) + Sync)) -> Result<usize> {
    let (row, things) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let row = scenes::table
            .find(lesson_id)
            .select(SceneRow::as_select())
            .first(conn)
            .optional()?
            .ok_or_else(|| Error::msg("That lesson has no scene."))?;
        (row, things(conn, lesson_id)?)
    };
    if things.is_empty() {
        return Err(Error::msg("The scene has no things left to show."));
    }
    let statements: Vec<Statement> = serde_json::from_str(&row.statements).unwrap_or_default();
    let arrangement: Vec<String> = statements.iter().map(|s| s.en.clone()).collect();
    let prompt = image_prompt(&row.description, &things, &arrangement);
    // Too few found everywhere: fewer than this many things, or all of them.
    let enough = MIN_SPOTS.min(things.len());
    let mut picture: Option<(Vec<u8>, u32, u32)> = None;
    if !redraw {
        if let Some(id) = row.image_file_id {
            let (_, bytes) = files::read(&mut db.lock(), id)?;
            picture = Some((bytes, u32::try_from(row.width).unwrap_or(0), u32::try_from(row.height).unwrap_or(0)));
        }
    }
    let mut best = 0;
    for _ in 0..DRAWINGS {
        let (jpeg, width, height) = match picture.take() {
            Some(kept) => kept,
            None => {
                set_status(&mut db.lock(), lesson_id, DRAWING, "")?;
                on_status(DRAWING);
                let (_, bytes, cost) = models.draw(&prompt).await?;
                let _ = crate::costs::record(&mut db.lock(), crate::costs::PICTURES, "openrouter", models.image_model(), cost, "scene");
                let (jpeg, width, height) = crate::pictures::fit(&bytes, SCENE_SIDE)?;
                store_picture(&mut db.lock(), lesson_id, models.image_model(), &prompt, &jpeg, width, height)?;
                (jpeg, width, height)
            }
        };
        set_status(&mut db.lock(), lesson_id, LOCATING, "")?;
        on_status(LOCATING);
        let ask = locate_prompt(&things);
        for _ in 0..LOOKS {
            let reply = models.look(&ask, "image/jpeg", &jpeg).await?;
            let _ = crate::costs::record_tokens(
                &mut db.lock(),
                crate::costs::SCENE_SPOTS,
                models.vision_model(),
                reply.cost,
                "scene",
                reply.tokens,
            );
            match parse_spots(&reply.text, things.len(), width, height) {
                Ok(found) if found.len() >= enough => {
                    store_spots(&mut db.lock(), lesson_id, models.vision_model(), &things, &found)?;
                    on_status(READY);
                    return Ok(found.len());
                }
                Ok(found) => best = best.max(found.len()),
                Err(reason) => crate::diagnostics::note("background", format!("Scene {lesson_id}: the boxes were no use ({reason})")),
            }
        }
    }
    Err(Error::msg(format!(
        "Only {best} of the {} things could be found in the picture. Try drawing it again.",
        things.len()
    )))
}

/// Scenes being made right now, so one is never made twice at once.
static MAKING: std::sync::Mutex<Vec<LessonId>> = std::sync::Mutex::new(Vec::new());

fn is_making(lesson_id: LessonId) -> bool {
    MAKING.lock().unwrap_or_else(|e| e.into_inner()).contains(&lesson_id)
}

/// Sent as `scenes://changed` as a scene moves on (drawing, locating,
/// ready, failed), so its page and anything showing it reload.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneChanged {
    pub lesson_id: LessonId,
    pub status: String,
}

/// Makes a scene in the background, unless it is being made already.
pub fn make_in_background(app: &tauri::AppHandle, lesson_id: LessonId, redraw: bool) {
    {
        let mut making = MAKING.lock().unwrap_or_else(|e| e.into_inner());
        if making.contains(&lesson_id) {
            return;
        }
        making.push(lesson_id);
    }
    let busy = crate::work::begin(app, "Drawing a scene…");
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _busy = busy;
        use tauri::{Emitter, Manager};
        let db = app.state::<Db>();
        let notify = |status: &str| {
            let _ = app.emit("scenes://changed", SceneChanged { lesson_id, status: status.to_string() });
        };
        // Chosen first, on its own: the lock must not be held while the
        // scene is made.
        let chosen = OpenRouter::chosen(&mut db.lock());
        let outcome = match chosen {
            Ok(models) => make(&db, lesson_id, redraw, &models, &notify).await,
            Err(error) => {
                let _ = set_status(&mut db.lock(), lesson_id, FAILED, &error.to_string());
                Err(error)
            }
        };
        MAKING.lock().unwrap_or_else(|e| e.into_inner()).retain(|id| *id != lesson_id);
        if let Err(error) = outcome {
            crate::diagnostics::note("background", format!("Making scene {lesson_id} failed: {error}"));
        }
        // Last, once it is no longer being made: the page shows the outcome.
        notify("done");
    });
}

/// Makes the scenes still waiting to be drawn or found — after the tutor
/// made a scene lesson, and at startup for one an earlier run did not
/// finish. Only scenes this device wrote last (`sync::written_here`): a
/// scene another device is making is that device's, or both would draw it
/// and pay twice.
pub fn resume_pending(app: &tauri::AppHandle) {
    use tauri::Manager;
    let pending: Vec<LessonId> = {
        let db = app.state::<Db>();
        let mut guard = db.lock();
        let conn = &mut *guard;
        let open: Vec<LessonId> = scenes::table
            .filter(scenes::status.eq_any([DRAWING, LOCATING]))
            .select(scenes::lesson_id)
            .load(conn)
            .unwrap_or_default();
        open.into_iter().filter(|id| crate::sync::written_here(conn, "scenes", &id.to_string())).collect()
    };
    for lesson_id in pending {
        make_in_background(app, lesson_id, false);
    }
}

/// Makes a lesson's scene again: the same picture's things found again, or
/// (`redraw`) a new picture. Also the retry after a failure.
#[tauri::command]
pub async fn scene_make(app: tauri::AppHandle, db: State<'_, Db>, lesson_id: LessonId, redraw: bool) -> Result<()> {
    {
        let mut guard = db.lock();
        let conn = &mut *guard;
        crate::ai::api_key(conn)?;
        let status = if redraw { DRAWING } else { LOCATING };
        let changed = diesel::update(scenes::table.find(lesson_id))
            .set((scenes::status.eq(status), scenes::error.eq(""), scenes::updated_at.eq(db::now_secs())))
            .execute(conn)?;
        if changed == 0 {
            return Err(Error::msg("That lesson has no scene."));
        }
    }
    make_in_background(&app, lesson_id, redraw);
    Ok(())
}

/// A scene as if drawn and found: a small picture, and each thing at the
/// box given (in order) — for the screens' shapes and their previews.
#[cfg(test)]
pub(crate) fn ready_for_test(conn: &mut Conn, lesson_id: LessonId, boxes: &[[f64; 4]]) -> Result<()> {
    let mut png = Vec::new();
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(400, 300, image::Rgb([250, 240, 220])))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| Error::msg(e.to_string()))?;
    let (jpeg, width, height) = crate::pictures::fit(&png, SCENE_SIDE)?;
    store_picture(conn, lesson_id, "test/image", "a test scene", &jpeg, width, height)?;
    let things = things(conn, lesson_id)?;
    let found: Vec<(usize, Area)> =
        boxes.iter().enumerate().map(|(i, [x0, y0, x1, y1])| (i + 1, Area { x: *x0, y: *y0, w: x1 - x0, h: y1 - y0 })).collect();
    store_spots(conn, lesson_id, "test/vision", &things, &found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(x: f64, y: f64, w: f64, h: f64) -> Area {
        Area { x, y, w, h }
    }

    #[test]
    fn boxes_are_read_as_fractions_and_nonsense_is_dropped() {
        let reply = r#"Here you go:
        ```json
        {"things": [
          {"n": 1, "found": true,  "x_min": 0.10, "y_min": 0.20, "x_max": 0.30, "y_max": 0.45},
          {"n": 2, "found": false, "x_min": 0, "y_min": 0, "x_max": 0, "y_max": 0},
          {"n": 3, "found": true,  "x_min": 0.50, "y_min": 0.50, "x_max": 0.51, "y_max": 0.70},
          {"n": 4, "found": true,  "x_min": 0.0, "y_min": 0.0, "x_max": 1.0, "y_max": 1.0},
          {"n": 5, "found": true,  "x_min": 0.9, "y_min": 0.1, "x_max": 1.4, "y_max": 0.3},
          {"n": 6, "found": true,  "x_min": 0.62, "y_min": 0.58, "x_max": 0.40, "y_max": 0.80},
          {"n": 7, "found": true,  "x_min": 0.70, "y_min": 0.10, "x_max": 1.02, "y_max": 0.30},
          {"n": 9, "found": true,  "x_min": 0.1, "y_min": 0.1, "x_max": 0.2, "y_max": 0.2},
          {"n": 1, "found": true,  "x_min": 0.6, "y_min": 0.6, "x_max": 0.7, "y_max": 0.7}
        ]}
        ```"#;
        let found = parse_spots(reply, 8, 1024, 768).unwrap();
        let numbers: Vec<usize> = found.iter().map(|(n, _)| *n).collect();
        // 2 not found, 3 a sliver, 4 the whole picture, 5 far off the edge,
        // 9 not one of the things, the second 1 a repeat.
        assert_eq!(numbers, [1, 6, 7]);
        let one = found[0].1;
        assert!((one.x - 0.10).abs() < 1e-9 && (one.w - 0.20).abs() < 1e-9 && (one.h - 0.25).abs() < 1e-9);
        // Corners given the wrong way round are put right; a little over
        // the edge is trimmed to it.
        assert!((found[1].1.x - 0.40).abs() < 1e-9 && (found[1].1.w - 0.22).abs() < 1e-9);
        assert!((found[2].1.x + found[2].1.w - 1.0).abs() < 1e-9);
    }

    #[test]
    fn thousandths_and_pixels_are_scaled() {
        let thousandths = r#"{"things": [{"n": 1, "found": true, "x_min": 100, "y_min": 200, "x_max": 300, "y_max": 450}]}"#;
        let found = parse_spots(thousandths, 2, 1024, 768).unwrap();
        assert!((found[0].1.x - 0.1).abs() < 1e-9 && (found[0].1.h - 0.25).abs() < 1e-9);
        let pixels = r#"{"things": [{"n": 2, "found": true, "x_min": 512, "y_min": 384, "x_max": 1024, "y_max": 768}]}"#;
        let found = parse_spots(pixels, 2, 2048, 1536).unwrap();
        assert_eq!(found[0].0, 2);
        assert!((found[0].1.x - 0.25).abs() < 1e-9 && (found[0].1.w - 0.25).abs() < 1e-9);
    }

    #[test]
    fn identical_or_doubled_boxes_are_not_trusted() {
        let same = r#"{"things": [
            {"n": 1, "found": true, "x_min": 0.2, "y_min": 0.2, "x_max": 0.4, "y_max": 0.4},
            {"n": 2, "found": true, "x_min": 0.2, "y_min": 0.2, "x_max": 0.4, "y_max": 0.4},
            {"n": 3, "found": true, "x_min": 0.21, "y_min": 0.2, "x_max": 0.4, "y_max": 0.4}]}"#;
        assert!(parse_spots(same, 3, 100, 100).is_err(), "the model did not look");
        let doubled = r#"{"things": [
            {"n": 1, "found": true, "x_min": 0.2, "y_min": 0.2, "x_max": 0.4, "y_max": 0.4},
            {"n": 2, "found": true, "x_min": 0.2, "y_min": 0.2, "x_max": 0.4, "y_max": 0.41},
            {"n": 3, "found": true, "x_min": 0.6, "y_min": 0.6, "x_max": 0.8, "y_max": 0.8}]}"#;
        let found = parse_spots(doubled, 3, 100, 100).unwrap();
        assert_eq!(found.iter().map(|(n, _)| *n).collect::<Vec<_>>(), [3], "two things in one box: neither");
        assert!(parse_spots("I can't see anything", 3, 100, 100).is_err());
        assert!(parse_spots(r#"{"boxes": []}"#, 3, 100, 100).is_err(), "another shape");
    }

    #[test]
    fn prepositions_are_checked_against_the_boxes() {
        let table = a(0.3, 0.4, 0.4, 0.3);
        let under = a(0.4, 0.62, 0.15, 0.15);
        let on = a(0.45, 0.3, 0.1, 0.12);
        let lamp = a(0.42, 0.05, 0.1, 0.1);
        let beside = a(0.75, 0.45, 0.15, 0.25);
        let inside = a(0.4, 0.45, 0.1, 0.1);
        assert!(holds("pod", &under, &table) && !holds("pod", &on, &table) && !holds("pod", &beside, &table));
        assert!(holds("na", &on, &table) && !holds("na", &under, &table));
        assert!(holds("nad", &lamp, &table) && !holds("nad", &under, &table));
        assert!(holds("obok", &beside, &table) && holds("przy", &beside, &table) && !holds("obok", &on, &table));
        assert!(holds("w", &inside, &table) && !holds("w", &beside, &table));
        assert!(!holds("za", &beside, &table), "a preposition the boxes cannot show is never borne out");
    }

    #[test]
    fn prompts_name_every_thing() {
        let things = vec![
            Thing { word_id: WordId::new(), pl: "zegar".into(), en: "clock".into() },
            Thing { word_id: WordId::new(), pl: "kot".into(), en: "cat, tomcat".into() },
        ];
        let draw = image_prompt("A sunny kitchen in the morning.", &things, &["The cat is under the table.".into()]);
        assert!(draw.contains("A sunny kitchen in the morning: These") || draw.contains("A sunny kitchen in the morning. These"));
        assert!(draw.contains("clock, cat.") && draw.contains("The cat is under the table.") && draw.contains("No text"));
        assert!(!draw.contains("zegar"), "no Polish in the picture prompt, or it may be written into the picture");
        let look = locate_prompt(&things);
        assert!(look.contains("1. clock (Polish: zegar)") && look.contains("2. cat (Polish: kot)"));
        let schema = spots_schema();
        assert_eq!(schema["properties"]["things"]["items"]["additionalProperties"], false);
    }
}

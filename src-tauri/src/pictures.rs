//! Course pictures: an image model draws what a course is about — not a
//! photo, a friendly illustration — so the course list and page are easier
//! to tell apart at a glance. The picture is a file like any other.

use diesel::prelude::*;
use tauri::State;

use crate::db::{Conn, Db};
use crate::error::{Error, Result};
use crate::files;
use crate::schema::{course_words, courses, lessons, words};
#[allow(unused_imports)]
use crate::ids::*;

/// What the picture should show, from the course's title, summary and words.
pub fn prompt(conn: &mut Conn, course_id: CourseId) -> Result<String> {
    let (title, title_pl, summary): (String, String, String) = courses::table
        .find(course_id)
        .select((courses::title_en, courses::title_pl, courses::summary_en))
        .first(conn)
        .optional()?
        .ok_or_else(|| Error::msg("That course no longer exists."))?;
    let words: Vec<String> = course_words::table
        .filter(course_words::course_id.eq(course_id))
        .order((course_words::lesson_id, course_words::ord))
        .select(course_words::en)
        .limit(12)
        .load(conn)?;
    let mut prompt = format!(
        "Draw a cheerful illustration for a language-learning course called \"{title}\" \
         (in Polish: \"{title_pl}\")."
    );
    if !summary.trim().is_empty() {
        prompt.push_str(&format!(" It is about: {}.", summary.trim().trim_end_matches('.')));
    }
    if !words.is_empty() {
        prompt.push_str(&format!(" Things from it to show: {}.", words.join(", ")));
    }
    prompt.push_str(
        " Style: a flat, colourful vector illustration with simple shapes and soft light, \
         a single scene, a hint of Poland where it fits. Square. No text, letters, words or \
         numbers anywhere in the image.",
    );
    Ok(prompt)
}

/// Draws a new picture for a course, replacing the old one. Returns its file
/// id (the webview shows it through the media protocol).
#[tauri::command]
pub async fn course_picture(app: tauri::AppHandle, db: State<'_, Db>, course_id: CourseId) -> Result<FileId> {
    set_status(&app, course_id, Status::Drawing);
    match draw(&db, course_id, false).await {
        Ok(Some(file_id)) => {
            set_status(&app, course_id, Status::Idle);
            Ok(file_id)
        }
        Ok(None) => Err(Error::msg("The picture was not kept.")),
        Err(error) => {
            set_status(&app, course_id, Status::Idle);
            Err(error)
        }
    }
}

/// Sent as `pictures://changed` when a course got a picture in the
/// background, so the screens showing it reload.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PictureChanged {
    pub course_id: CourseId,
    pub file_id: FileId,
}

/// Whether a course's picture is being drawn, or why it could not be:
/// `course_picture_status`, and sent as `pictures://status` whenever it
/// changes, so the course's page shows the drawing and a failure.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PictureStatus {
    pub course_id: CourseId,
    pub drawing: bool,
    /// Why the last drawing in this run failed; none once one succeeds.
    pub error: Option<String>,
}

enum Status {
    Idle,
    Drawing,
    Failed(String),
}

/// Course pictures being drawn now, and the last failure of each, in this
/// run only: a picture still wanted is drawn again at the next start.
static STATUS: std::sync::Mutex<Vec<PictureStatus>> = std::sync::Mutex::new(Vec::new());

fn status_of(course_id: CourseId) -> PictureStatus {
    STATUS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|s| s.course_id == course_id)
        .cloned()
        .unwrap_or(PictureStatus { course_id, drawing: false, error: None })
}

fn set_status(app: &tauri::AppHandle, course_id: CourseId, status: Status) {
    use tauri::Emitter;
    let status = match status {
        Status::Idle => PictureStatus { course_id, drawing: false, error: None },
        Status::Drawing => PictureStatus { course_id, drawing: true, error: None },
        Status::Failed(error) => PictureStatus { course_id, drawing: false, error: Some(error) },
    };
    {
        let mut all = STATUS.lock().unwrap_or_else(|e| e.into_inner());
        all.retain(|s| s.course_id != course_id);
        if status.drawing || status.error.is_some() {
            all.push(status.clone());
        }
    }
    let _ = app.emit("pictures://status", status);
}

/// Whether a course's picture is being drawn right now, or why the last
/// drawing failed.
#[tauri::command]
pub fn course_picture_status(course_id: CourseId) -> PictureStatus {
    status_of(course_id)
}

/// Records that a new course wants a picture: work that outlives this run,
/// drawn by [`draw_wanted_in_background`] at the next start if this run
/// ends first. Not for a folder of one-offs (Other lessons), nor a course
/// with a picture. Returns whether it was marked.
pub fn want(conn: &mut Conn, course_id: CourseId) -> Result<bool> {
    let changed = diesel::update(
        courses::table
            .find(course_id)
            .filter(courses::image_file_id.is_null())
            .filter(courses::slug.ne_all(crate::courses::OPEN_ENDED)),
    )
    .set(courses::picture_wanted_at.eq(crate::db::now_secs()))
    .execute(conn)?;
    Ok(changed > 0)
}

/// Draws a new course's picture in the background — every course the tutor
/// makes gets one without asking (see [`want`]). Kept only while it is still
/// wanted: a picture the learner deleted, or drew themselves meanwhile, is
/// not replaced. A failure is logged and shown on the course's page, which
/// can try again; the course is there either way.
pub fn draw_in_background(app: &tauri::AppHandle, course_id: CourseId) {
    if status_of(course_id).drawing {
        return;
    }
    set_status(app, course_id, Status::Drawing);
    let busy = crate::work::begin(app, "Drawing a course picture…");
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _busy = busy;
        use tauri::{Emitter, Manager};
        let db = app.state::<Db>();
        match draw(&db, course_id, true).await {
            Ok(Some(file_id)) => {
                set_status(&app, course_id, Status::Idle);
                let _ = app.emit("pictures://changed", PictureChanged { course_id, file_id });
            }
            Ok(None) => set_status(&app, course_id, Status::Idle),
            Err(error) => {
                crate::diagnostics::note("background", format!("Drawing course {course_id}'s picture failed: {error}"));
                set_status(&app, course_id, Status::Failed(error.to_string()));
            }
        }
    });
}

/// The courses whose picture is wanted and not drawn yet ([`want`]): only
/// those this device wrote last (`sync::written_here`) — a course another
/// device made is that device's to draw, or both would draw it and pay
/// twice — and never a folder of one-offs.
pub fn wanted(conn: &mut Conn) -> Vec<CourseId> {
    let all: Vec<CourseId> = courses::table
        .filter(courses::picture_wanted_at.is_not_null())
        .filter(courses::image_file_id.is_null())
        .filter(courses::slug.ne_all(crate::courses::OPEN_ENDED))
        .select(courses::id)
        .load(conn)
        .unwrap_or_default();
    all.into_iter().filter(|id| crate::sync::written_here(conn, "courses", &id.to_string())).collect()
}

/// At startup: draws the course pictures an earlier run was asked for and
/// did not finish (the app closed while the tutor or the drawing was busy).
pub fn draw_wanted_in_background(app: &tauri::AppHandle) {
    use tauri::Manager;
    let courses = wanted(&mut app.state::<Db>().lock());
    for course_id in courses {
        draw_in_background(app, course_id);
    }
}

/// Draws a course's picture and puts it in place of the old one, which is
/// deleted. With `only_if_wanted` (the background), the picture is kept
/// only if the course still wants one; otherwise `None`. The lock is not
/// held while drawing.
async fn draw(db: &Db, course_id: CourseId, only_if_wanted: bool) -> Result<Option<FileId>> {
    let (key, model, prompt) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        (crate::ai::api_key(conn)?, crate::ai::image_model(conn), prompt(conn, course_id)?)
    };
    let (mime, bytes, cost) = crate::ai::generate_image(&key, &model, &prompt).await?;
    let mut guard = db.lock();
    let conn = &mut *guard;
    let _ = crate::costs::record(conn, crate::costs::PICTURES, "openrouter", &model, cost, "");
    conn.transaction(|conn| {
        let (old, wanted): (Option<FileId>, Option<i64>) = courses::table
            .find(course_id)
            .select((courses::image_file_id, courses::picture_wanted_at))
            .first(conn)
            .optional()?
            .ok_or_else(|| Error::msg("That course no longer exists."))?;
        if only_if_wanted && (wanted.is_none() || old.is_some()) {
            return Ok(None);
        }
        let name = format!("course-{course_id}-{}.{}", crate::db::now_secs(), files::extension_for(&mime));
        let id = files::store(conn, files::COURSE_IMAGE, &name, &mime, &bytes)?;
        diesel::update(courses::table.find(course_id))
            .set((courses::image_file_id.eq(id), courses::picture_wanted_at.eq(None::<i64>)))
            .execute(conn)?;
        if let Some(old) = old {
            files::delete(conn, &[old])?;
        }
        Ok(Some(id))
    })
}

/// Removes a course's picture. A picture the tutor asked for and not drawn
/// yet is no longer wanted either: what the learner deletes is never drawn
/// again behind their back.
#[tauri::command]
pub async fn course_picture_delete(app: tauri::AppHandle, db: State<'_, Db>, course_id: CourseId) -> Result<()> {
    {
        let mut guard = db.lock();
        guard.transaction(|conn| {
            let old: Option<FileId> = courses::table.find(course_id).select(courses::image_file_id).first(conn)?;
            diesel::update(courses::table.find(course_id))
                .set((courses::image_file_id.eq(None::<FileId>), courses::picture_wanted_at.eq(None::<i64>)))
                .execute(conn)?;
            files::delete(conn, &old.into_iter().collect::<Vec<_>>()).map(|_| ())
        })?;
    }
    if !status_of(course_id).drawing {
        set_status(&app, course_id, Status::Idle);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Word pictures: picture lessons
// ---------------------------------------------------------------------------

/// How every word picture is drawn, so a lesson's pictures read as one set.
pub const WORD_STYLE: &str = "Style: a flat, colourful vector illustration with simple shapes and soft \
    light. Show only this one object, whole and centred, on a plain light background: no other \
    objects, no scene, no people unless the object cannot be shown without them. Square. No text, \
    letters, words or numbers anywhere in the image.";

/// The side, in pixels, a word picture is shrunk to: sharp on a phone,
/// small in the database.
const WORD_IMAGE_SIDE: u32 = 512;

/// How many word pictures are drawn at once.
const PARALLEL_DRAWINGS: usize = 3;

/// What a word's picture shows. Made from the word alone (and the fixed
/// style), so drawing it again asks for the same thing.
pub fn word_prompt(word: &str, english: &str) -> String {
    format!(
        "Draw one everyday object for a Polish vocabulary card: {english} (in Polish: \"{word}\"). It \
         must be recognisable at a glance as exactly this, and not as anything similar. {WORD_STYLE}"
    )
}

/// Shrinks a picture to fit [`WORD_IMAGE_SIDE`] and stores it as JPEG, on
/// white where it was transparent.
pub fn shrink(bytes: &[u8]) -> Result<Vec<u8>> {
    Ok(fit(bytes, WORD_IMAGE_SIDE)?.0)
}

/// Shrinks a picture to fit a square of `side` pixels (a smaller one is
/// kept as it is) and returns it as JPEG, on white where it was
/// transparent, with its width and height.
pub fn fit(bytes: &[u8], side: u32) -> Result<(Vec<u8>, u32, u32)> {
    let picture = image::load_from_memory(bytes).map_err(|e| Error::msg(format!("Could not read the picture: {e}")))?;
    let picture = if picture.width() > side || picture.height() > side { picture.thumbnail(side, side) } else { picture };
    let picture = picture.into_rgba8();
    let mut flat = image::RgbImage::new(picture.width(), picture.height());
    for (x, y, pixel) in picture.enumerate_pixels() {
        let [r, g, b, a] = pixel.0;
        let over = |c: u8| u8::try_from((u32::from(c) * u32::from(a) + 255 * (255 - u32::from(a))) / 255).unwrap_or(u8::MAX);
        flat.put_pixel(x, y, image::Rgb([over(r), over(g), over(b)]));
    }
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85)
        .encode_image(&flat)
        .map_err(|e| Error::msg(format!("Could not store the picture: {e}")))?;
    Ok((out, flat.width(), flat.height()))
}

/// Words being drawn right now, so one is never drawn twice at once.
static DRAWING: std::sync::Mutex<Vec<WordId>> = std::sync::Mutex::new(Vec::new());

/// Draws a word's picture, replacing the one it had. Returns its file id.
pub async fn draw_word(db: &Db, word_id: WordId) -> Result<FileId> {
    let (key, model, prompt) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let (word, english): (String, String) = words::table
            .find(word_id)
            .select((words::pl, words::en))
            .first(conn)
            .optional()?
            .ok_or_else(|| Error::msg("That word no longer exists."))?;
        (crate::ai::api_key(conn)?, crate::ai::image_model(conn), word_prompt(&word, &english))
    };
    let (_, bytes, cost) = crate::ai::generate_image(&key, &model, &prompt).await?;
    let small = shrink(&bytes)?;
    let mut guard = db.lock();
    let conn = &mut *guard;
    let _ = crate::costs::record(conn, crate::costs::PICTURES, "openrouter", &model, cost, "word");
    let name = format!("word-{word_id}-{}.jpg", crate::db::now_secs());
    let id = files::store(conn, files::WORD_IMAGE, &name, "image/jpeg", &small)?;
    let old: Option<Option<FileId>> = words::table.find(word_id).select(words::image_file_id).first(conn).optional()?;
    let Some(old) = old else {
        // The word went while it was drawn.
        files::delete(conn, &[id])?;
        return Err(Error::msg("That word no longer exists."));
    };
    diesel::update(words::table.find(word_id)).set(words::image_file_id.eq(id)).execute(conn)?;
    if let Some(old) = old {
        files::delete(conn, &[old])?;
    }
    Ok(id)
}

/// Sent as `pictures://word` when a word got (or lost) its picture.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WordPictureChanged {
    pub lesson_id: LessonId,
    pub word_id: WordId,
    pub file_id: Option<FileId>,
}

/// The words of a picture lesson still without a picture (none for any
/// other type: only picture lessons have pictures).
pub fn missing(conn: &mut Conn, lesson_id: LessonId) -> Result<Vec<WordId>> {
    let lesson_type: String = lessons::table.find(lesson_id).select(lessons::lesson_type).first(conn)?;
    if lesson_type != "pictures" {
        return Ok(Vec::new());
    }
    Ok(words::table
        .filter(words::lesson_id.eq(lesson_id))
        .filter(words::image_file_id.is_null())
        .order((words::ord, words::id))
        .select(words::id)
        .load(conn)?)
}

/// Draws the given words' pictures in the background, a few at a time, each
/// announced as it lands. Words already being drawn are skipped.
pub fn draw_words_in_background(app: &tauri::AppHandle, lesson_id: LessonId, word_ids: Vec<WordId>) {
    let word_ids: Vec<WordId> = {
        let mut drawing = DRAWING.lock().unwrap_or_else(|e| e.into_inner());
        let fresh: Vec<WordId> = word_ids.into_iter().filter(|id| !drawing.contains(id)).collect();
        drawing.extend(&fresh);
        fresh
    };
    if word_ids.is_empty() {
        return;
    }
    let busy = crate::work::begin(app, "Drawing pictures…");
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _busy = busy;
        use futures_util::stream::{self, StreamExt};
        use tauri::{Emitter, Manager};
        let db = app.state::<Db>();
        let db: &Db = &db;
        stream::iter(word_ids)
            .for_each_concurrent(PARALLEL_DRAWINGS, |word_id| {
                let app = app.clone();
                async move {
                    match draw_word(db, word_id).await {
                        Ok(file_id) => {
                            let _ = app.emit("pictures://word", WordPictureChanged { lesson_id, word_id, file_id: Some(file_id) });
                        }
                        Err(error) => crate::diagnostics::note("background", format!("Drawing word {word_id}'s picture failed: {error}")),
                    }
                    DRAWING.lock().unwrap_or_else(|e| e.into_inner()).retain(|id| *id != word_id);
                }
            })
            .await;
    });
}

/// Draws whatever picture lessons still lack — after the tutor made one, and
/// at startup for drawings an earlier run did not finish.
///
/// Only lessons whose current version this device wrote
/// (`sync::written_here`): a lesson synced from another device that is
/// still drawing its pictures would otherwise get every picture twice, one
/// from each device, and pay for both.
pub fn draw_missing_in_background(app: &tauri::AppHandle) {
    use tauri::Manager;
    let db = app.state::<Db>();
    let lessons: Vec<LessonId> = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let all: Vec<LessonId> =
            lessons::table.filter(lessons::lesson_type.eq("pictures")).select(lessons::id).load(conn).unwrap_or_default();
        all.into_iter().filter(|id| crate::sync::written_here(conn, "lessons", &id.to_string())).collect()
    };
    for lesson_id in lessons {
        let missing = missing(&mut db.lock(), lesson_id).unwrap_or_default();
        if !missing.is_empty() {
            draw_words_in_background(app, lesson_id, missing);
        }
    }
}

/// Draws a picture lesson's missing pictures in the background; returns how
/// many are on the way.
#[tauri::command]
pub async fn lesson_add_pictures(app: tauri::AppHandle, db: State<'_, Db>, lesson_id: LessonId) -> Result<usize> {
    let missing = {
        let mut guard = db.lock();
        crate::ai::api_key(&mut guard)?;
        missing(&mut guard, lesson_id)?
    };
    let count = missing.len();
    draw_words_in_background(&app, lesson_id, missing);
    Ok(count)
}

/// Draws one word's picture again (or for the first time).
#[tauri::command]
pub async fn word_picture_draw(db: State<'_, Db>, word_id: WordId) -> Result<FileId> {
    draw_word(&db, word_id).await
}

/// Removes a word's picture; it is practised without one.
#[tauri::command]
pub async fn word_picture_delete(db: State<'_, Db>, word_id: WordId) -> Result<()> {
    let mut guard = db.lock();
    let conn = &mut *guard;
    let old: Option<FileId> = words::table.find(word_id).select(words::image_file_id).first(conn)?;
    diesel::update(words::table.find(word_id)).set(words::image_file_id.eq(None::<FileId>)).execute(conn)?;
    files::delete(conn, &old.into_iter().collect::<Vec<_>>())?;
    Ok(())
}

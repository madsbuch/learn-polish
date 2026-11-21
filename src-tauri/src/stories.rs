//! Story lessons' stories: a short illustrated story told panel by panel —
//! a picture and one to three Polish sentences under it, and a question
//! after each panel. The tutor writes it (create_story_lesson): who is in
//! it, with a short description of how each one looks, and its panels.
//!
//! The text is there at once, so the story can be read straight away; the
//! pictures are drawn in the background, one panel after another, with the
//! same image model as every other picture in the app. Every panel's prompt
//! carries every character's description and one shared style line, so the
//! same girl in the same red coat walks through the whole story. A panel
//! whose picture fails says why and can be drawn again on its own; the
//! others carry on.
//!
//! Only the device that wrote a panel draws it (`sync::written_here`):
//! another device shows "being drawn" until the picture syncs over.

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
use crate::schema::{course_words, stories, story_panels, words};
use crate::singalong::GlossWord;

/// Panels in a story: long enough to be a story, short enough for one sitting.
// Spec: PIC-6
pub const MIN_PANELS: usize = 4;
pub const MAX_PANELS: usize = 8;
/// Sentences under one panel.
pub const MAX_LINES: usize = 3;
/// People (or animals) in a story: few enough to tell apart and keep the same.
pub const MAX_CHARACTERS: usize = 4;
/// New words a story brings, and its sentences kept to practise.
pub const MAX_WORDS: usize = 12;
pub const MAX_PRACTISED: usize = 4;
/// The longest side a panel's picture is kept at.
const PANEL_SIDE: u32 = 1024;
/// Wider than tall, so the picture and its sentences fit a phone together.
const ASPECT: &str = "4:3";

pub const WAITING: &str = "waiting";
pub const DRAWING: &str = "drawing";
pub const READY: &str = "ready";
pub const FAILED: &str = "failed";

/// How every panel is drawn, so a story's pictures read as one book — and
/// as the app's other pictures.
pub const STORY_STYLE: &str = "Style: a flat, colourful vector illustration with simple shapes and soft \
    light, like a picture book, the same in every panel of the story. A hint of Poland where it fits. \
    Landscape, 4:3. No text, letters, words, numbers or speech bubbles anywhere in the image.";

// ---------------------------------------------------------------------------
// What is stored
// ---------------------------------------------------------------------------

/// Someone in the story, and how they look in every picture.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Character {
    pub name: String,
    /// A short description of how they look: "a girl of about ten with a red
    /// raincoat and two dark plaits".
    pub look: String,
    /// "f" or "m": the voice their lines are read in.
    #[serde(default)]
    pub gender: String,
}

/// One sentence under a panel, as stored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Line {
    pub pl: String,
    pub en: String,
    /// The character who says it; empty for narration.
    #[serde(default)]
    pub speaker: String,
}

/// The question after a panel, as stored: in English on a low lesson, in
/// Polish on harder ones (the tutor writes it so).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Question {
    pub prompt: String,
    pub answer: String,
    pub wrong: Vec<String>,
}

/// A sentence as the screens show it: with the voice to read it in.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StoryLine {
    pub pl: String,
    pub en: String,
    /// Who says it; empty for narration.
    pub speaker: String,
    /// "f" or "m" for a character's line; empty for the narrator.
    pub gender: String,
}

/// A panel as the screens show it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StoryPanel {
    pub id: PanelId,
    /// Its place in the story, from 1.
    pub number: i64,
    /// What its picture shows, in English (said to screen readers, and
    /// shown while it is being drawn).
    pub picture: String,
    /// Its picture, once drawn.
    pub image: Option<FileId>,
    pub width: i64,
    pub height: i64,
    /// waiting | drawing | ready | failed
    pub status: String,
    /// Why its picture failed, when it did.
    pub error: String,
    pub lines: Vec<StoryLine>,
}

/// A word of the story's lesson, for its last page: "Words met".
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StoryWord {
    pub word_id: WordId,
    pub pl: String,
    pub en: String,
    /// Met for the first time in this story.
    pub new: bool,
}

/// What a story card shows: a panel to read (with the meanings of its
/// words), the panel a question is about, or the story's words at the end.
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct StoryCard {
    pub title: String,
    pub panel: Option<StoryPanel>,
    /// Panels in the story.
    pub total: i64,
    /// A question's kind: choice | picture (which picture shows…) | order
    /// (which came first); empty on a panel or the end.
    pub ask: String,
    /// The meanings of the panel's words (tap a word).
    pub glossary: Vec<GlossWord>,
    /// The end: the lesson's words, met in the story.
    pub words: Vec<StoryWord>,
}

/// A story lesson's story as its page shows it: the picture book.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryInfo {
    pub theme: String,
    pub characters: Vec<Character>,
    pub panels: Vec<StoryPanel>,
    /// The meanings of every word in it.
    pub glossary: Vec<GlossWord>,
    /// Pictures are being drawn on this device right now (another device's
    /// story in the making shows its panels' status, but only that device
    /// carries on).
    pub working: bool,
}

/// A whole story, loaded: its panels with their questions.
#[derive(Debug, Clone)]
pub struct Story {
    pub theme: String,
    pub characters: Vec<Character>,
    pub panels: Vec<(StoryPanel, Question)>,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = story_panels, check_for_backend(diesel::sqlite::Sqlite))]
struct PanelRow {
    id: PanelId,
    ord: i64,
    picture: String,
    lines: String,
    question: String,
    image_file_id: Option<FileId>,
    width: i64,
    height: i64,
    status: String,
    error: String,
}

fn voice_of(characters: &[Character], speaker: &str) -> String {
    if speaker.trim().is_empty() {
        return String::new();
    }
    characters
        .iter()
        .find(|c| same(&c.name, speaker))
        .map(|c| if c.gender == "m" { "m" } else { "f" }.to_string())
        .unwrap_or_default()
}

fn panel_of(row: &PanelRow, characters: &[Character]) -> StoryPanel {
    let lines: Vec<Line> = serde_json::from_str(&row.lines).unwrap_or_default();
    StoryPanel {
        id: row.id,
        number: row.ord + 1,
        picture: row.picture.clone(),
        image: if row.status == READY { row.image_file_id } else { None },
        width: row.width,
        height: row.height,
        status: row.status.clone(),
        error: row.error.clone(),
        lines: lines
            .into_iter()
            .map(|l| StoryLine { gender: voice_of(characters, &l.speaker), pl: l.pl, en: l.en, speaker: l.speaker })
            .collect(),
    }
}

/// A lesson's story; None for a lesson without one.
pub fn load(conn: &mut Conn, lesson_id: LessonId) -> Result<Option<Story>> {
    let Some((theme, characters)) = stories::table
        .find(lesson_id)
        .select((stories::theme, stories::characters))
        .first::<(String, String)>(conn)
        .optional()?
    else {
        return Ok(None);
    };
    let characters: Vec<Character> = serde_json::from_str(&characters).unwrap_or_default();
    let rows: Vec<PanelRow> = story_panels::table
        .filter(story_panels::lesson_id.eq(lesson_id))
        .order((story_panels::ord, story_panels::id))
        .select(PanelRow::as_select())
        .load(conn)?;
    let panels = rows
        .iter()
        .map(|row| (panel_of(row, &characters), serde_json::from_str(&row.question).unwrap_or_default()))
        .collect();
    Ok(Some(Story { theme, characters, panels }))
}

/// The meanings of a story's words: its lesson's own words first (marked
/// `lesson`, the words it teaches), then its course's, then any the learner
/// has — matched to their forms in the story ("kota" is "kot").
pub fn glossary(conn: &mut Conn, lesson_id: LessonId, story: &Story) -> Result<Vec<GlossWord>> {
    let text: Vec<&str> = story.panels.iter().flat_map(|(p, _)| p.lines.iter().map(|l| l.pl.as_str())).collect();
    let forms = crate::singalong::forms_in(text);
    let own: Vec<(WordId, String, String)> = words::table
        .filter(words::lesson_id.eq(lesson_id))
        .order((words::ord, words::id))
        .select((words::id, words::pl, words::en))
        .load(conn)?;
    let mut known: Vec<crate::singalong::Known> = own.into_iter().map(|(id, pl, en)| (pl, en, Some(id), true)).collect();
    let courses: Vec<CourseId> = crate::schema::course_lessons::table
        .filter(crate::schema::course_lessons::lesson_id.eq(lesson_id))
        .select(crate::schema::course_lessons::course_id)
        .load(conn)?;
    let course: Vec<(WordId, String, String)> = course_words::table
        .filter(course_words::course_id.eq_any(courses))
        .order(course_words::ord)
        .select((course_words::id, course_words::pl, course_words::en))
        .load(conn)?;
    known.extend(course.into_iter().map(|(id, pl, en)| (pl, en, Some(id), false)));
    crate::singalong::meanings(conn, &forms, known)
}

/// The glossary entries for one panel's words.
pub fn glossary_for(glossary: &[GlossWord], panel: &StoryPanel) -> Vec<GlossWord> {
    let forms = crate::singalong::forms_in(panel.lines.iter().map(|l| l.pl.as_str()));
    glossary.iter().filter(|g| forms.contains(&g.form)).cloned().collect()
}

/// A lesson's story for its page; None for a lesson without one.
pub fn info(conn: &mut Conn, lesson_id: LessonId) -> Result<Option<StoryInfo>> {
    let Some(story) = load(conn, lesson_id)? else {
        return Ok(None);
    };
    let glossary = glossary(conn, lesson_id, &story)?;
    Ok(Some(StoryInfo {
        working: is_drawing(lesson_id),
        theme: story.theme,
        characters: story.characters,
        panels: story.panels.into_iter().map(|(p, _)| p).collect(),
        glossary,
    }))
}

// ---------------------------------------------------------------------------
// Prompts
// ---------------------------------------------------------------------------

/// The characters a panel is about: named in what it shows, or speaking in
/// it.
fn in_panel<'a>(characters: &'a [Character], picture: &str, lines: &[Line]) -> Vec<&'a Character> {
    let picture = picture.to_lowercase();
    characters
        .iter()
        .filter(|c| picture.contains(&c.name.to_lowercase()) || lines.iter().any(|l| same(&l.speaker, &c.name)))
        .collect()
}

/// What the image model is asked to draw for a panel: what it shows, where,
/// every character's look (the same words in every panel, so they stay the
/// same people), and the story's one style — so drawing it again asks for
/// the same thing.
pub fn panel_prompt(number: usize, total: usize, picture: &str, setting: &str, characters: &[Character], lines: &[Line]) -> String {
    let mut prompt = format!(
        "Draw picture {number} of {total} of an illustrated children's story for a Polish lesson. This picture shows: {}.",
        picture.trim().trim_end_matches('.')
    );
    if !setting.trim().is_empty() {
        prompt.push_str(&format!(" The story takes place: {}.", setting.trim().trim_end_matches('.')));
    }
    if !characters.is_empty() {
        let cast = characters.iter().map(|c| format!("{} — {}", c.name, c.look.trim().trim_end_matches('.'))).collect::<Vec<_>>().join("; ");
        prompt.push_str(&format!(
            " The characters of the story look exactly like this in every picture (same face, hair, \
             clothes and colours): {cast}."
        ));
        let here = in_panel(characters, picture, lines);
        if !here.is_empty() {
            let names = here.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", ");
            prompt.push_str(&format!(" In this picture: {names}."));
        }
    }
    prompt.push(' ');
    prompt.push_str(STORY_STYLE);
    prompt
}

// ---------------------------------------------------------------------------
// Making a story lesson
// ---------------------------------------------------------------------------

/// A sentence as the tutor writes it.
#[derive(Debug, Clone, Deserialize)]
pub struct NewLine {
    pub pl: String,
    pub en: String,
    #[serde(default)]
    pub speaker: String,
    /// Keep it as one of the lesson's sentences, to practise like any other.
    #[serde(default)]
    pub practise: bool,
}

/// A panel as the tutor writes it.
#[derive(Debug, Clone, Deserialize)]
pub struct NewPanel {
    /// What the picture shows, in English.
    pub picture: String,
    pub lines: Vec<NewLine>,
    pub question: Question,
}

/// A story lesson, as the tutor asks for one.
#[derive(Debug, Clone, Deserialize)]
pub struct NewStory {
    pub title: String,
    #[serde(default)]
    pub summary: String,
    /// What the story is about, in English.
    pub theme: String,
    #[serde(default)]
    pub setting: String,
    #[serde(default)]
    pub complexity: Option<String>,
    pub characters: Vec<Character>,
    pub panels: Vec<NewPanel>,
    /// The words it teaches: new words become the lesson's words.
    #[serde(default)]
    pub words: Vec<NewWord>,
    #[serde(default, alias = "topic_id")]
    pub course_id: Option<CourseId>,
    #[serde(default)]
    pub period_days: Option<i64>,
}

fn same(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// Whether a word (or phrase) is in the story, in any of its forms.
fn in_story(word: &str, text: &str, forms: &[String]) -> bool {
    let word = word.trim().to_lowercase();
    if word.contains(' ') {
        return text.contains(&word);
    }
    forms.iter().any(|f| crate::singalong::same_word(f, &word))
}

/// Checks a story before anything is written, so every mistake comes back to
/// the tutor at once: the things to fix, not a half-made lesson.
pub fn check(input: &NewStory) -> Result<()> {
    let mut problems: Vec<String> = Vec::new();
    if input.title.trim().is_empty() {
        problems.push("Give the story a title.".into());
    }
    if input.theme.trim().is_empty() {
        problems.push("Say what the story is about (theme), in English.".into());
    }
    let c = input.characters.len();
    if !(1..=MAX_CHARACTERS).contains(&c) {
        problems.push(format!("A story has 1–{MAX_CHARACTERS} characters (got {c})."));
    }
    for (i, ch) in input.characters.iter().enumerate() {
        if ch.name.trim().is_empty() || ch.look.trim().is_empty() {
            problems.push("Every character needs a name and a short description of how they look (look), for the pictures.".into());
        }
        if !ch.gender.is_empty() && ch.gender != "f" && ch.gender != "m" {
            problems.push(format!("{}: gender is \"f\" or \"m\" (the voice their lines are read in).", ch.name));
        }
        if input.characters[..i].iter().any(|other| same(&other.name, &ch.name)) {
            problems.push(format!("Two characters are called {}.", ch.name));
        }
    }
    let n = input.panels.len();
    if !(MIN_PANELS..=MAX_PANELS).contains(&n) {
        problems.push(format!("A story has {MIN_PANELS}–{MAX_PANELS} panels (got {n})."));
    }
    let mut practised = 0;
    for (i, panel) in input.panels.iter().enumerate() {
        let at = format!("Panel {}", i + 1);
        if panel.picture.trim().is_empty() {
            problems.push(format!("{at}: say what its picture shows (picture), in English."));
        }
        if !(1..=MAX_LINES).contains(&panel.lines.len()) {
            problems.push(format!("{at}: 1–{MAX_LINES} sentences under each picture (got {}).", panel.lines.len()));
        }
        for line in &panel.lines {
            if line.pl.trim().is_empty() || line.en.trim().is_empty() {
                problems.push(format!("{at}: every sentence needs its Polish and its English."));
            }
            if !line.speaker.trim().is_empty() && !input.characters.iter().any(|c| same(&c.name, &line.speaker)) {
                problems.push(format!(
                    "{at}: “{}” is said by {}, who is not one of the characters (leave speaker empty for narration).",
                    line.pl, line.speaker
                ));
            }
            practised += usize::from(line.practise);
        }
        let q = &panel.question;
        let wrong: Vec<&String> = q.wrong.iter().filter(|w| !w.trim().is_empty()).collect();
        if q.prompt.trim().is_empty() || q.answer.trim().is_empty() {
            problems.push(format!("{at}: its question needs a prompt and an answer."));
        } else if wrong.is_empty() || wrong.len() > 3 {
            problems.push(format!("{at}: give its question 1–3 wrong answers."));
        } else if wrong.iter().any(|w| same(w, &q.answer)) || wrong.iter().enumerate().any(|(j, w)| wrong[..j].iter().any(|o| same(o, w))) {
            problems.push(format!("{at}: the question's answers must all differ (the right one is not among the wrong ones)."));
        }
    }
    if practised > MAX_PRACTISED {
        problems.push(format!("Mark at most {MAX_PRACTISED} sentences to practise (got {practised})."));
    }
    let text: Vec<String> = input.panels.iter().flat_map(|p| p.lines.iter().map(|l| l.pl.to_lowercase())).collect();
    let forms = crate::singalong::forms_in(text.iter().map(String::as_str));
    let text = text.join(" ");
    if input.words.is_empty() || input.words.len() > MAX_WORDS {
        problems.push(format!("List the 1–{MAX_WORDS} words the story teaches (got {}).", input.words.len()));
    }
    let mut unseen: Vec<&str> = Vec::new();
    for (i, word) in input.words.iter().enumerate() {
        if word.pl.trim().is_empty() || word.en.trim().is_empty() {
            problems.push("Every word needs its Polish and its English.".into());
        } else if !in_story(&word.pl, &text, &forms) {
            unseen.push(word.pl.trim());
        }
        if input.words[..i].iter().any(|other| same(&other.pl, &word.pl)) {
            problems.push(format!("“{}” is listed twice.", word.pl));
        }
    }
    // Polish words change shape ("pies", "psa"; "spać", "śpi"), so a word not
    // found as written may still be there: only a list that is mostly not
    // in the story is sent back.
    if unseen.len() * 3 > input.words.len() {
        problems.push(format!(
            "These words are not in the story: {} — list the words its sentences use.",
            unseen.join(", ")
        ));
    }
    problems.dedup();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(Error::msg(format!("The story was not made. Fix these and try again:\n- {}", problems.join("\n- "))))
    }
}

/// Makes a story lesson: its words (and the sentences marked to practise)
/// become the lesson's own, practised and reviewed like any others; its
/// panels are there to read at once, and wait for their pictures —
/// [`resume_pending`] draws them in the background.
pub fn create(conn: &mut Conn, input: NewStory, origin: &str) -> Result<Lesson> {
    check(&input)?;
    let complexity = input.complexity.as_deref().unwrap_or("low");
    let course_id = match input.course_id {
        Some(id) => id,
        None => crate::courses::other_course(conn)?,
    };
    let sentences = input
        .panels
        .iter()
        .flat_map(|p| p.lines.iter())
        .filter(|l| l.practise)
        .map(|l| crate::courses::NewSentence { pl: l.pl.clone(), en: l.en.clone(), ..Default::default() })
        .collect();
    let items = crate::courses::Items { words: input.words.clone(), sentences, dialogue: vec![] };
    let (lesson, _) = crate::lessons::create_drill(
        conn,
        course_id,
        &input.title,
        &input.summary,
        "story",
        complexity,
        &items,
        input.period_days,
        origin,
    )?;
    let clean = crate::text::clean;
    let characters: Vec<Character> = input
        .characters
        .iter()
        .map(|c| Character { name: clean(&c.name), look: clean(&c.look), gender: if c.gender == "m" { "m" } else { "f" }.into() })
        .collect();
    let now = db::now_secs();
    diesel::insert_into(stories::table)
        .values((
            stories::lesson_id.eq(lesson.id),
            stories::theme.eq(clean(&input.theme)),
            stories::setting.eq(clean(&input.setting)),
            stories::characters.eq(serde_json::to_string(&characters)?),
            stories::created_at.eq(now),
            stories::updated_at.eq(now),
        ))
        .execute(conn)?;
    for (ord, panel) in input.panels.iter().enumerate() {
        let lines: Vec<Line> = panel
            .lines
            .iter()
            .map(|l| {
                let speaker = characters.iter().find(|c| same(&c.name, &l.speaker)).map(|c| c.name.clone()).unwrap_or_default();
                Line { pl: clean(&l.pl), en: clean(&l.en), speaker }
            })
            .collect();
        let question = Question {
            prompt: clean(&panel.question.prompt),
            answer: clean(&panel.question.answer),
            wrong: panel.question.wrong.iter().map(|w| clean(w)).filter(|w| !w.is_empty()).collect(),
        };
        diesel::insert_into(story_panels::table)
            .values((
                story_panels::id.eq(PanelId::new()),
                story_panels::lesson_id.eq(lesson.id),
                story_panels::ord.eq(crate::convert::count(ord)),
                story_panels::picture.eq(clean(&panel.picture)),
                story_panels::lines.eq(serde_json::to_string(&lines)?),
                story_panels::question.eq(serde_json::to_string(&question)?),
                story_panels::status.eq(WAITING),
                story_panels::updated_at.eq(now),
            ))
            .execute(conn)?;
    }
    Ok(lesson)
}

// ---------------------------------------------------------------------------
// Drawing the panels
// ---------------------------------------------------------------------------

/// The image model a story is drawn with: the real one on OpenRouter, or a
/// stand-in in tests.
pub trait Painter: Sync {
    /// Draws a picture: its type, bytes and cost.
    fn draw(&self, prompt: &str) -> impl Future<Output = Result<(String, Vec<u8>, Option<f64>)>> + Send;
    fn model(&self) -> &str;
}

/// The learner's chosen image model on OpenRouter.
pub struct OpenRouter {
    key: String,
    model: String,
}

impl OpenRouter {
    pub fn chosen(conn: &mut Conn) -> Result<Self> {
        Ok(Self { key: crate::ai::api_key(conn)?, model: crate::ai::image_model(conn) })
    }
}

impl Painter for OpenRouter {
    fn draw(&self, prompt: &str) -> impl Future<Output = Result<(String, Vec<u8>, Option<f64>)>> + Send {
        crate::ai::generate_image_shaped(&self.key, &self.model, prompt, Some(ASPECT))
    }

    fn model(&self) -> &str {
        &self.model
    }
}

fn set_status(conn: &mut Conn, panel: PanelId, status: &str, error: &str) -> Result<()> {
    diesel::update(story_panels::table.find(panel))
        .set((story_panels::status.eq(status), story_panels::error.eq(error), story_panels::updated_at.eq(db::now_secs())))
        .execute(conn)?;
    Ok(())
}

/// Keeps a newly drawn picture as a panel's, in place of its old one.
fn store_picture(conn: &mut Conn, panel: PanelId, model: &str, prompt: &str, jpeg: &[u8], width: u32, height: u32) -> Result<FileId> {
    conn.transaction(|conn| {
        let old: Option<FileId> = story_panels::table.find(panel).select(story_panels::image_file_id).first(conn)?;
        let name = format!("story-{panel}-{}.jpg", db::now_secs());
        let id = files::store(conn, files::STORY_IMAGE, &name, "image/jpeg", jpeg)?;
        diesel::update(story_panels::table.find(panel))
            .set((
                story_panels::image_file_id.eq(id),
                story_panels::width.eq(i64::from(width)),
                story_panels::height.eq(i64::from(height)),
                story_panels::prompt.eq(prompt),
                story_panels::image_model.eq(model),
                story_panels::status.eq(READY),
                story_panels::error.eq(""),
                story_panels::updated_at.eq(db::now_secs()),
            ))
            .execute(conn)?;
        if let Some(old) = old {
            files::delete(conn, &[old])?;
        }
        Ok(id)
    })
}

/// The next panel to draw: the first waiting one, or one left drawing by an
/// earlier run that this run has not tried yet.
fn next_to_draw(conn: &mut Conn, lesson_id: LessonId, tried: &[PanelId]) -> Result<Option<(PanelId, String)>> {
    let open: Vec<(PanelId, String)> = story_panels::table
        .filter(story_panels::lesson_id.eq(lesson_id))
        .filter(story_panels::status.eq_any([WAITING, DRAWING]))
        .order((story_panels::ord, story_panels::id))
        .select((story_panels::id, story_panels::status))
        .load(conn)?;
    Ok(open.into_iter().find(|(id, status)| status == WAITING || !tried.contains(id)))
}

/// What a panel is drawn from: its prompt, made fresh from the story.
fn prompt_for(conn: &mut Conn, lesson_id: LessonId, panel: PanelId) -> Result<String> {
    let (setting, characters): (String, String) =
        stories::table.find(lesson_id).select((stories::setting, stories::characters)).first(conn)?;
    let characters: Vec<Character> = serde_json::from_str(&characters).unwrap_or_default();
    let (ord, picture, lines): (i64, String, String) =
        story_panels::table.find(panel).select((story_panels::ord, story_panels::picture, story_panels::lines)).first(conn)?;
    let total: i64 = story_panels::table.filter(story_panels::lesson_id.eq(lesson_id)).count().get_result(conn)?;
    let lines: Vec<Line> = serde_json::from_str(&lines).unwrap_or_default();
    let number = usize::try_from(ord + 1).unwrap_or(1);
    Ok(panel_prompt(number, usize::try_from(total).unwrap_or(number), &picture, &setting, &characters, &lines))
}

/// How a drawing run went: pictures drawn, and panels that failed.
#[derive(Debug, Default, PartialEq)]
pub struct Drawn {
    pub drawn: usize,
    pub failed: usize,
}

/// Draws a story's waiting panels, one after another in story order, so the
/// first pages are ready first. A panel that fails is marked so, with its
/// reason, and the next is drawn; a panel set waiting again while this runs
/// (try again) is drawn before it ends. Every step is written as it happens
/// and reported through `on_change`, so the page can show it.
pub async fn draw_panels(
    db: &Db,
    lesson_id: LessonId,
    painter: &impl Painter,
    on_change: &(dyn Fn(PanelId, &str) + Sync),
) -> Result<Drawn> {
    let mut tried: Vec<PanelId> = Vec::new();
    let mut outcome = Drawn::default();
    loop {
        let (panel, prompt) = {
            let mut guard = db.lock();
            let conn = &mut *guard;
            let Some((panel, _)) = next_to_draw(conn, lesson_id, &tried)? else { break };
            let prompt = prompt_for(conn, lesson_id, panel)?;
            set_status(conn, panel, DRAWING, "")?;
            (panel, prompt)
        };
        tried.push(panel);
        on_change(panel, DRAWING);
        let drawn = match painter.draw(&prompt).await {
            Ok((_, bytes, cost)) => {
                let _ = crate::costs::record(&mut db.lock(), crate::costs::PICTURES, "openrouter", painter.model(), cost, "story");
                crate::pictures::fit(&bytes, PANEL_SIDE)
                    .and_then(|(jpeg, w, h)| store_picture(&mut db.lock(), panel, painter.model(), &prompt, &jpeg, w, h))
            }
            Err(error) => Err(error),
        };
        match drawn {
            Ok(_) => {
                outcome.drawn += 1;
                on_change(panel, READY);
            }
            Err(error) => {
                outcome.failed += 1;
                set_status(&mut db.lock(), panel, FAILED, &error.to_string())?;
                on_change(panel, FAILED);
            }
        }
    }
    Ok(outcome)
}

/// Stories being drawn right now, so one is never drawn twice at once.
static DRAWING_NOW: std::sync::Mutex<Vec<LessonId>> = std::sync::Mutex::new(Vec::new());

fn is_drawing(lesson_id: LessonId) -> bool {
    DRAWING_NOW.lock().unwrap_or_else(|e| e.into_inner()).contains(&lesson_id)
}

/// Sent as `stories://changed` as a panel's picture moves on (drawing,
/// ready, failed), and with status "done" when a run ends, so the lesson
/// page and the picture book reload.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryChanged {
    pub lesson_id: LessonId,
    pub panel_id: Option<PanelId>,
    pub status: String,
}

/// Panels of a lesson waiting for a picture that this device should draw.
pub(crate) fn pending(conn: &mut Conn, lesson_id: Option<LessonId>) -> Vec<(LessonId, PanelId)> {
    let mut query = story_panels::table
        .filter(story_panels::status.eq_any([WAITING, DRAWING]))
        .select((story_panels::lesson_id, story_panels::id))
        .into_boxed();
    if let Some(lesson_id) = lesson_id {
        query = query.filter(story_panels::lesson_id.eq(lesson_id));
    }
    let open: Vec<(LessonId, PanelId)> = query.load(conn).unwrap_or_default();
    open.into_iter().filter(|(_, id)| crate::sync::written_here(conn, "story_panels", &id.to_string())).collect()
}

/// Draws a story's panels in the background, unless that is going on already.
pub fn draw_in_background(app: &tauri::AppHandle, lesson_id: LessonId) {
    {
        let mut drawing = DRAWING_NOW.lock().unwrap_or_else(|e| e.into_inner());
        if drawing.contains(&lesson_id) {
            return;
        }
        drawing.push(lesson_id);
    }
    let busy = crate::work::begin(app, "Drawing a story…");
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _busy = busy;
        use tauri::{Emitter, Manager};
        let db = app.state::<Db>();
        let notify = |panel_id: Option<PanelId>, status: &str| {
            let _ = app.emit("stories://changed", StoryChanged { lesson_id, panel_id, status: status.to_string() });
        };
        // Chosen first, on its own: the lock must not be held while drawing.
        let chosen = OpenRouter::chosen(&mut db.lock());
        let outcome = match chosen {
            Ok(painter) => draw_panels(&db, lesson_id, &painter, &|panel, status| notify(Some(panel), status)).await,
            Err(error) => {
                // No key (or no model): every waiting panel says so, to try again later.
                let mut guard = db.lock();
                for (_, panel) in pending(&mut guard, Some(lesson_id)) {
                    let _ = set_status(&mut guard, panel, FAILED, &error.to_string());
                }
                Err(error)
            }
        };
        DRAWING_NOW.lock().unwrap_or_else(|e| e.into_inner()).retain(|id| *id != lesson_id);
        match outcome {
            Err(error) => crate::diagnostics::note("background", format!("Drawing story {lesson_id} failed: {error}")),
            Ok(Drawn { failed, .. }) if failed > 0 => {
                crate::diagnostics::note("background", format!("Story {lesson_id}: {failed} panel(s) could not be drawn"))
            }
            Ok(_) => {}
        }
        // Last, once it is no longer drawing: the page shows the outcome.
        notify(None, "done");
        // A panel set to try again just as this run ended is drawn now.
        let again = !pending(&mut db.lock(), Some(lesson_id)).is_empty();
        if again && is_key_set(&db) {
            draw_in_background(&app, lesson_id);
        }
    });
}

fn is_key_set(db: &Db) -> bool {
    crate::ai::api_key(&mut db.lock()).is_ok()
}

/// Draws the panels still waiting for a picture — after the tutor made a
/// story, and at startup for one an earlier run did not finish. Only panels
/// this device wrote last (`sync::written_here`): a story another device is
/// drawing is that device's, or both would draw it and pay twice.
pub fn resume_pending(app: &tauri::AppHandle) {
    use tauri::Manager;
    let lessons: Vec<LessonId> = {
        let db = app.state::<Db>();
        let mut guard = db.lock();
        let mut lessons: Vec<LessonId> = Vec::new();
        for (lesson, _) in pending(&mut guard, None) {
            if !lessons.contains(&lesson) {
                lessons.push(lesson);
            }
        }
        lessons
    };
    for lesson_id in lessons {
        draw_in_background(app, lesson_id);
    }
}

/// Sets a story's panels to be drawn (again): one panel, or (`panel_id`
/// None) every one that failed. Returns how many.
pub fn redraw(conn: &mut Conn, lesson_id: LessonId, panel_id: Option<PanelId>) -> Result<usize> {
    let mut query = diesel::update(story_panels::table)
        .filter(story_panels::lesson_id.eq(lesson_id))
        .into_boxed();
    query = match panel_id {
        Some(id) => query.filter(story_panels::id.eq(id)),
        None => query.filter(story_panels::status.eq(FAILED)),
    };
    let changed = query
        .set((story_panels::status.eq(WAITING), story_panels::error.eq(""), story_panels::updated_at.eq(db::now_secs())))
        .execute(conn)?;
    if changed == 0 && panel_id.is_some() {
        return Err(Error::msg("That panel is not in this story."));
    }
    Ok(changed)
}

/// Draws a story's panel again (or every failed one): the retry after a
/// failure, or a new picture for one the learner does not like.
#[tauri::command]
pub async fn story_draw(app: tauri::AppHandle, db: State<'_, Db>, lesson_id: LessonId, panel_id: Option<PanelId>) -> Result<()> {
    {
        let mut guard = db.lock();
        let conn = &mut *guard;
        crate::ai::api_key(conn)?;
        redraw(conn, lesson_id, panel_id)?;
    }
    draw_in_background(&app, lesson_id);
    Ok(())
}

/// A story's panels as if drawn: a small picture each (a different colour
/// per panel) — for the screens' shapes and their previews.
#[cfg(test)]
pub(crate) fn ready_for_test(conn: &mut Conn, lesson_id: LessonId) -> Result<()> {
    let panels: Vec<PanelId> =
        story_panels::table.filter(story_panels::lesson_id.eq(lesson_id)).order(story_panels::ord).select(story_panels::id).load(conn)?;
    for (n, panel) in panels.into_iter().enumerate() {
        let shade = u8::try_from(n * 30 % 200).unwrap_or(0);
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(400, 300, image::Rgb([250, 200 - shade / 2, 120 + shade / 2])))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(|e| Error::msg(e.to_string()))?;
        let (jpeg, width, height) = crate::pictures::fit(&png, PANEL_SIDE)?;
        store_picture(conn, panel, "test/image", "a test panel", &jpeg, width, height)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ola() -> Character {
        Character { name: "Ola".into(), look: "a girl of about ten with a red raincoat and two dark plaits".into(), gender: "f".into() }
    }

    #[test]
    fn every_panel_prompt_carries_the_characters_and_the_style() {
        let burek = Character { name: "Burek".into(), look: "a small brown dog with one white ear".into(), gender: "m".into() };
        let cast = [ola(), burek];
        let lines = [Line { pl: "Gdzie jest Burek?".into(), en: "Where is Burek?".into(), speaker: "Ola".into() }];
        let one = panel_prompt(1, 5, "Ola looks for her dog in the rain.", "a small Polish town in autumn", &cast, &lines);
        let five = panel_prompt(5, 5, "The dog sleeps by the stove.", "a small Polish town in autumn", &cast, &[]);
        for prompt in [&one, &five] {
            assert!(prompt.contains("Ola — a girl of about ten with a red raincoat and two dark plaits"), "{prompt}");
            assert!(prompt.contains("Burek — a small brown dog with one white ear"), "{prompt}");
            assert!(prompt.contains(STORY_STYLE) && prompt.contains("a small Polish town in autumn"), "{prompt}");
            assert!(!prompt.contains("Gdzie"), "no Polish in a picture prompt, or it may be written into the picture");
        }
        assert!(one.contains("picture 1 of 5") && one.contains("In this picture: Ola."), "{one}");
        assert!(!five.contains("In this picture"), "nobody named: all of them described, none singled out");
    }

    #[test]
    fn a_speakers_voice_is_their_gender_and_narration_has_none() {
        let cast = [ola(), Character { name: "Tata".into(), look: "a tall man".into(), gender: "m".into() }];
        assert_eq!(voice_of(&cast, "tata"), "m");
        assert_eq!(voice_of(&cast, "Ola"), "f");
        assert_eq!(voice_of(&cast, ""), "");
        assert!(in_story("kot", "mój kot śpi", &crate::singalong::forms_in(["mój kotem śpi"])));
        assert!(in_story("dzień dobry", "dzień dobry, ola!", &[]));
    }
}

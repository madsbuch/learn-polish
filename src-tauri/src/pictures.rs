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
pub async fn course_picture(db: State<'_, Db>, course_id: CourseId) -> Result<FileId> {
    draw(&db, course_id).await
}

/// Sent as `pictures://changed` when a course got a picture in the
/// background, so the screens showing it reload.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PictureChanged {
    pub course_id: CourseId,
    pub file_id: FileId,
}

/// Draws a new course's picture in the background — every course gets one
/// without asking. A failure (no key, a model that draws nothing) is only
/// logged: the course is there either way, and its page can try again.
pub fn draw_in_background(app: &tauri::AppHandle, course_id: CourseId) {
    {
        // A folder of one-offs (Other lessons) gets no picture.
        use tauri::Manager;
        let slug: Option<String> =
            courses::table.find(course_id).select(courses::slug).first(&mut *app.state::<Db>().lock()).optional().ok().flatten();
        if slug.is_some_and(|s| crate::courses::OPEN_ENDED.contains(&s.as_str())) {
            return;
        }
    }
    let busy = crate::work::begin(app, "Drawing a course picture…");
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _busy = busy;
        use tauri::{Emitter, Manager};
        let db = app.state::<Db>();
        match draw(&db, course_id).await {
            Ok(file_id) => {
                let _ = app.emit("pictures://changed", PictureChanged { course_id, file_id });
            }
            Err(error) => crate::diagnostics::note("background", format!("Drawing course {course_id}'s picture failed: {error}")),
        }
    });
}

async fn draw(db: &Db, course_id: CourseId) -> Result<FileId> {
    let (key, model, prompt) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        (crate::ai::api_key(conn)?, crate::ai::image_model(conn), prompt(conn, course_id)?)
    };
    let (mime, bytes, cost) = crate::ai::generate_image(&key, &model, &prompt).await?;
    let mut guard = db.lock();
    let conn = &mut *guard;
    let _ = crate::costs::record(conn, crate::costs::PICTURES, "openrouter", &model, cost, "");
    let name = format!("course-{course_id}-{}.{}", crate::db::now_secs(), files::extension_for(&mime));
    let id = files::store(conn, files::COURSE_IMAGE, &name, &mime, &bytes)?;
    let old: Option<FileId> = courses::table.find(course_id).select(courses::image_file_id).first(conn)?;
    diesel::update(courses::table.find(course_id)).set(courses::image_file_id.eq(id)).execute(conn)?;
    if let Some(old) = old {
        files::delete(conn, &[old])?;
    }
    Ok(id)
}

/// Removes a course's picture.
#[tauri::command]
pub async fn course_picture_delete(db: State<'_, Db>, course_id: CourseId) -> Result<()> {
    let mut guard = db.lock();
    let conn = &mut *guard;
    let old: Option<FileId> = courses::table.find(course_id).select(courses::image_file_id).first(conn)?;
    diesel::update(courses::table.find(course_id)).set(courses::image_file_id.eq(None::<FileId>)).execute(conn)?;
    files::delete(conn, &old.into_iter().collect::<Vec<_>>())?;
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
    let picture = image::load_from_memory(bytes).map_err(|e| Error::msg(format!("Could not read the picture: {e}")))?;
    let picture = picture.thumbnail(WORD_IMAGE_SIDE, WORD_IMAGE_SIDE).into_rgba8();
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
    Ok(out)
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
pub fn draw_missing_in_background(app: &tauri::AppHandle) {
    use tauri::Manager;
    let db = app.state::<Db>();
    let lessons: Vec<LessonId> = {
        let mut guard = db.lock();
        lessons::table
            .filter(lessons::lesson_type.eq("pictures"))
            .select(lessons::id)
            .load(&mut *guard)
            .unwrap_or_default()
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

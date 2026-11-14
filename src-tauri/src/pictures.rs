//! Course pictures: an image model draws what a course is about — not a
//! photo, a friendly illustration — so the course list and page are easier
//! to tell apart at a glance. The picture is a file like any other.

use diesel::prelude::*;
use tauri::State;

use crate::db::{Conn, Db};
use crate::error::{Error, Result};
use crate::files;
use crate::schema::{course_words, topics};

/// What the picture should show, from the course's title, summary and words.
pub fn prompt(conn: &mut Conn, topic_id: i64) -> Result<String> {
    let (title, title_pl, summary): (String, String, String) = topics::table
        .find(topic_id)
        .select((topics::title_en, topics::title_pl, topics::summary_en))
        .first(conn)
        .optional()?
        .ok_or_else(|| Error::msg("That course no longer exists."))?;
    let words: Vec<String> = course_words::table
        .filter(course_words::topic_id.eq(topic_id))
        .order((course_words::block_id, course_words::ord))
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
pub async fn course_picture(db: State<'_, Db>, topic_id: i64) -> Result<i64> {
    draw(&db, topic_id).await
}

/// Sent as `pictures://changed` when a course got a picture in the
/// background, so the screens showing it reload.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PictureChanged {
    pub topic_id: i64,
    pub file_id: i64,
}

/// Draws a new course's picture in the background — every course gets one
/// without asking. A failure (no key, a model that draws nothing) is only
/// logged: the course is there either way, and its page can try again.
pub fn draw_in_background(app: &tauri::AppHandle, topic_id: i64) {
    let busy = crate::work::begin(app, "Drawing a course picture…");
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _busy = busy;
        use tauri::{Emitter, Manager};
        let db = app.state::<Db>();
        match draw(&db, topic_id).await {
            Ok(file_id) => {
                let _ = app.emit("pictures://changed", PictureChanged { topic_id, file_id });
            }
            Err(error) => eprintln!("course picture for {topic_id}: {error}"),
        }
    });
}

async fn draw(db: &Db, topic_id: i64) -> Result<i64> {
    let (key, model, prompt) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        (crate::ai::api_key(conn)?, crate::ai::image_model(conn), prompt(conn, topic_id)?)
    };
    let (mime, bytes, cost) = crate::ai::generate_image(&key, &model, &prompt).await?;
    let mut guard = db.lock();
    let conn = &mut *guard;
    let _ = crate::costs::record(conn, crate::costs::PICTURES, "openrouter", &model, cost, "");
    let name = format!("course-{topic_id}-{}.{}", crate::db::now_secs(), files::extension_for(&mime));
    let id = files::store(conn, files::COURSE_IMAGE, &name, &mime, &bytes)?;
    let old: Option<i64> = topics::table.find(topic_id).select(topics::image_file_id).first(conn)?;
    diesel::update(topics::table.find(topic_id)).set(topics::image_file_id.eq(id)).execute(conn)?;
    if let Some(old) = old {
        files::delete(conn, &[old])?;
    }
    Ok(id)
}

/// Removes a course's picture.
#[tauri::command]
pub async fn course_picture_delete(db: State<'_, Db>, topic_id: i64) -> Result<()> {
    let mut guard = db.lock();
    let conn = &mut *guard;
    let old: Option<i64> = topics::table.find(topic_id).select(topics::image_file_id).first(conn)?;
    diesel::update(topics::table.find(topic_id)).set(topics::image_file_id.eq(None::<i64>)).execute(conn)?;
    files::delete(conn, &old.into_iter().collect::<Vec<_>>())?;
    Ok(())
}

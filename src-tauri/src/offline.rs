//! Courses kept offline: a course the learner marks is made ready for use
//! without a connection, and kept ready as it grows.
//!
//! Ready means every asset its lessons use is in the database: the audio of
//! everything they say (speech.rs: words, sentences, dialogue lines, a
//! story's sentences, a scene's, the sound pairs' words) and every picture
//! they show (a picture lesson's words, a scene, a story's panels). Songs
//! and Wikipedia sources are stored when they are made, so they are there
//! already. What cannot be had offline is said, not hidden: a conversation
//! lesson (study, listening, writing) talks to a model, and making audio or
//! pictures takes the learner's key.
//!
//! The mark is the course's `offline_at` and syncs with it; the assets are
//! files, which sync too, so a course made ready on one device is ready on
//! the others once they have synced.
//!
//! Downloading is asked for, never silent: switching it on, or "Download
//! now", fetches everything and tries again what failed before. By itself
//! — at startup, and after the tutor adds to a course — the app only
//! fetches missing audio; pictures are drawn by the device that made the
//! lesson (pictures.rs, scenes.rs, stories.rs), as they always were, so two
//! synced devices never both pay for one.

use diesel::prelude::*;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::db::{Conn, Db};
use crate::error::Result;
use crate::ids::*;
use crate::schema::{course_lessons, courses, lessons, scenes, story_panels, words};

/// How ready a course is for use without a connection.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OfflineStatus {
    pub course_id: CourseId,
    /// Whether the learner asked for it to be kept offline.
    pub enabled: bool,
    /// Everything its lessons say, and how much of it is stored.
    pub clips: i64,
    pub clips_stored: i64,
    /// Every picture its lessons show, and how many are drawn.
    pub pictures: i64,
    pub pictures_stored: i64,
    /// Lessons that are conversations with a model: they need a connection
    /// whatever is downloaded.
    pub online_lessons: i64,
    /// Every clip and picture is stored.
    pub ready: bool,
    /// A download is running now.
    pub working: bool,
    /// Whether there is a key to download with.
    pub can_download: bool,
}

/// Sent as `offline://changed` when a course's download starts or ends.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfflineChanged {
    pub course_id: CourseId,
}

/// Courses being downloaded right now, so one is never downloaded twice at once.
static WORKING: std::sync::Mutex<Vec<CourseId>> = std::sync::Mutex::new(Vec::new());

fn is_working(course_id: CourseId) -> bool {
    WORKING.lock().unwrap_or_else(|e| e.into_inner()).contains(&course_id)
}

/// A course's lessons with their types.
fn lessons_of(conn: &mut Conn, course_id: CourseId) -> Result<Vec<(LessonId, String)>> {
    Ok(course_lessons::table
        .inner_join(lessons::table)
        .filter(course_lessons::course_id.eq(course_id))
        .order(course_lessons::ord)
        .select((lessons::id, lessons::lesson_type))
        .load(conn)?)
}

/// What a course still lacks in pictures: picture-lesson words without one,
/// scenes that are not ready, stories with panels not drawn.
#[derive(Debug, Default, PartialEq)]
struct Pictures {
    total: i64,
    stored: i64,
    /// Picture lessons and the words of each still without a picture.
    words: Vec<(LessonId, Vec<WordId>)>,
    scenes: Vec<LessonId>,
    stories: Vec<LessonId>,
}

fn pictures_of(conn: &mut Conn, course_id: CourseId) -> Result<Pictures> {
    let mut found = Pictures::default();
    for (lesson_id, lesson_type) in lessons_of(conn, course_id)? {
        if lesson_type == "pictures" {
            let total: i64 = words::table.filter(words::lesson_id.eq(lesson_id)).count().get_result(conn)?;
            let missing = crate::pictures::missing(conn, lesson_id)?;
            found.total += total;
            found.stored += total - crate::convert::count(missing.len());
            if !missing.is_empty() {
                found.words.push((lesson_id, missing));
            }
        }
        // A scene or a story, whatever the type is called: by its rows.
        let scene: Option<(String, Option<FileId>)> =
            scenes::table.find(lesson_id).select((scenes::status, scenes::image_file_id)).first(conn).optional()?;
        if let Some((status, image)) = scene {
            found.total += 1;
            if status == crate::scenes::READY && image.is_some() {
                found.stored += 1;
            } else {
                found.scenes.push(lesson_id);
            }
        }
        let panels: Vec<(String, Option<FileId>)> = story_panels::table
            .filter(story_panels::lesson_id.eq(lesson_id))
            .select((story_panels::status, story_panels::image_file_id))
            .load(conn)?;
        let drawn = panels.iter().filter(|(status, image)| status == crate::stories::READY && image.is_some()).count();
        found.total += crate::convert::count(panels.len());
        found.stored += crate::convert::count(drawn);
        if drawn < panels.len() {
            found.stories.push(lesson_id);
        }
    }
    Ok(found)
}

pub fn status(conn: &mut Conn, course_id: CourseId) -> Result<OfflineStatus> {
    let enabled: Option<i64> = courses::table.find(course_id).select(courses::offline_at).first(conn)?;
    let audio = crate::speech::course_audio_in(conn, course_id)?;
    let pictures = pictures_of(conn, course_id)?;
    let online_lessons = lessons_of(conn, course_id)?
        .iter()
        .filter(|(_, lesson_type)| crate::lesson_types::get(lesson_type).is_ok_and(|t| !t.is_drill()))
        .count();
    let clips = crate::convert::count(audio.len());
    let clips_stored = crate::convert::count(audio.iter().filter(|item| item.stored).count());
    Ok(OfflineStatus {
        course_id,
        enabled: enabled.is_some(),
        clips,
        clips_stored,
        pictures: pictures.total,
        pictures_stored: pictures.stored,
        online_lessons: crate::convert::count(online_lessons),
        ready: clips_stored == clips && pictures.stored == pictures.total,
        working: is_working(course_id),
        can_download: crate::ai::api_key(conn).is_ok(),
    })
}

/// Marks a course to be kept offline, or takes the mark away. Taking it
/// away deletes nothing: what was downloaded stays (Settings → Audio clears
/// audio no course uses).
pub fn set(conn: &mut Conn, course_id: CourseId, on: bool) -> Result<()> {
    let already: Option<i64> = courses::table.find(course_id).select(courses::offline_at).first(conn)?;
    if already.is_some() == on {
        return Ok(());
    }
    diesel::update(courses::table.find(course_id))
        .set(courses::offline_at.eq(on.then(crate::db::now_secs)))
        .execute(conn)?;
    Ok(())
}

/// The courses the learner keeps offline.
pub fn kept(conn: &mut Conn) -> Result<Vec<CourseId>> {
    Ok(courses::table.filter(courses::offline_at.is_not_null()).order(courses::ord).select(courses::id).load(conn)?)
}

/// Downloads what a course lacks, in the background, unless that is going
/// on already. Its audio always; with `everything` (the learner asked) its
/// pictures too, those that failed before included. Without a key there is
/// nothing to download with, and nothing happens.
pub fn download_in_background(app: &AppHandle, course_id: CourseId, everything: bool) {
    let db = app.state::<Db>();
    let (lines, pictures) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        if crate::ai::api_key(conn).is_err() {
            return;
        }
        let lines: Vec<crate::speech::SpeechLine> = crate::speech::course_audio_in(conn, course_id)
            .unwrap_or_default()
            .into_iter()
            .filter(|item| !item.stored)
            .map(|item| crate::speech::SpeechLine { text: item.text, gender: item.gender })
            .collect();
        let pictures = if everything { pictures_of(conn, course_id).unwrap_or_default() } else { Pictures::default() };
        // Panels that failed are set to be drawn again: the learner asked.
        for lesson_id in &pictures.stories {
            let _ = crate::stories::redraw(conn, *lesson_id, None);
        }
        (lines, pictures)
    };
    // Each of these is its own piece of background work, announced as it
    // lands; none holds the database while it draws.
    for (lesson_id, word_ids) in pictures.words {
        crate::pictures::draw_words_in_background(app, lesson_id, word_ids);
    }
    for lesson_id in pictures.scenes {
        crate::scenes::make_in_background(app, lesson_id, false);
    }
    for lesson_id in pictures.stories {
        crate::stories::draw_in_background(app, lesson_id);
    }
    if lines.is_empty() {
        let _ = app.emit("offline://changed", OfflineChanged { course_id });
        return;
    }
    {
        let mut working = WORKING.lock().unwrap_or_else(|e| e.into_inner());
        if working.contains(&course_id) {
            return;
        }
        working.push(course_id);
    }
    let busy = crate::work::begin(app, "Downloading a course…");
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _busy = busy;
        let _ = app.emit("offline://changed", OfflineChanged { course_id });
        let db = app.state::<Db>();
        crate::speech::fill(&app, &db, lines, false).await;
        WORKING.lock().unwrap_or_else(|e| e.into_inner()).retain(|id| *id != course_id);
        let _ = app.emit("offline://changed", OfflineChanged { course_id });
    });
}

/// Fetches the audio the courses kept offline have gained since they were
/// last complete — at startup, and after the tutor adds to a course.
pub fn keep_up_in_background(app: &AppHandle) {
    let courses = kept(&mut app.state::<Db>().lock()).unwrap_or_default();
    for course_id in courses {
        download_in_background(app, course_id, false);
    }
}

#[tauri::command]
pub async fn course_offline(db: State<'_, Db>, course_id: CourseId) -> Result<OfflineStatus> {
    status(&mut db.lock(), course_id)
}

/// Switches keeping a course offline on or off; on, the download starts.
#[tauri::command]
pub async fn course_offline_set(app: AppHandle, db: State<'_, Db>, course_id: CourseId, on: bool) -> Result<OfflineStatus> {
    set(&mut db.lock(), course_id, on)?;
    if on {
        download_in_background(&app, course_id, true);
    }
    status(&mut db.lock(), course_id)
}

/// "Download now": everything the course lacks, failed pictures tried again.
#[tauri::command]
pub async fn course_offline_download(app: AppHandle, db: State<'_, Db>, course_id: CourseId) -> Result<OfflineStatus> {
    download_in_background(&app, course_id, true);
    status(&mut db.lock(), course_id)
}

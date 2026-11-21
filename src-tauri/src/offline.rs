//! Courses kept offline: a course the learner marks is made ready for use
//! without a connection, and kept ready as it grows.
//!
//! Ready means everything its lessons use that is not a conversation is in
//! the database: the audio of everything they say (speech.rs: words,
//! sentences, dialogue lines, a story's sentences, a scene's, the sound
//! pairs' words, the example sentences), each at normal speed and slowly;
//! the example sentences a wrong answer shows for each word (examples.rs);
//! and every picture they show (a picture lesson's words, a scene, a
//! story's panels). Songs and Wikipedia sources are stored when they are
//! made, so they are there already. What cannot be had offline is said, not
//! hidden: a conversation lesson (study, listening, writing) talks to a
//! model, and making audio, examples or pictures takes the learner's key.
//!
//! The mark is the course's `offline_at` and syncs with it; the assets are
//! files, which sync too, so a course made ready on one device is ready on
//! the others once they have synced.
//!
//! Switching it on, or "Download now", fetches everything and tries again
//! what failed before. By itself — at startup, and after the tutor adds to
//! a course — the app fetches missing example sentences and audio; pictures
//! are drawn by the device that made the lesson (pictures.rs, scenes.rs,
//! stories.rs), as they always were, so two synced devices never both pay
//! for one.
//!
//! On Android a download shows its course and progress in the notification
//! that keeps the app working (work.rs), and leaving the app does not stop
//! it. Should Android stop the app anyway, a job carries on where it left
//! off without the app being opened (background.rs).

mod background;

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
    /// Everything its lessons say, at normal speed and slowly, and how
    /// much of it is stored.
    pub clips: i64,
    pub clips_stored: i64,
    /// Its words, and how many have their example sentences.
    pub examples: i64,
    pub examples_stored: i64,
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
    /// Example sentences are missing and no chat model is chosen to write
    /// them with.
    pub needs_model: bool,
}

/// Sent as `offline://changed` when a course's download starts or ends.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfflineChanged {
    pub course_id: CourseId,
}

/// How many words' example sentences are written at once.
const PARALLEL_EXAMPLES: usize = 3;

/// Courses being downloaded right now, so one is never downloaded twice at
/// once — by the app or by the job in the background (background.rs), which
/// run in the same process.
static WORKING: std::sync::Mutex<Vec<CourseId>> = std::sync::Mutex::new(Vec::new());

/// Courses whose latest download in this process ran to its end, failures
/// and all: the job in the background leaves them be. A new process (the
/// app was stopped mid-download) has none, so it carries on with them.
static FINISHED: std::sync::Mutex<Vec<CourseId>> = std::sync::Mutex::new(Vec::new());

fn is_working(course_id: CourseId) -> bool {
    WORKING.lock().unwrap_or_else(|e| e.into_inner()).contains(&course_id)
}

fn any_working() -> bool {
    !WORKING.lock().unwrap_or_else(|e| e.into_inner()).is_empty()
}

/// Takes a course to download; false if a download of it runs already.
fn claim(course_id: CourseId) -> bool {
    let mut working = WORKING.lock().unwrap_or_else(|e| e.into_inner());
    if working.contains(&course_id) {
        return false;
    }
    working.push(course_id);
    FINISHED.lock().unwrap_or_else(|e| e.into_inner()).retain(|id| *id != course_id);
    true
}

/// A course's download has run to its end.
fn release(course_id: CourseId) {
    WORKING.lock().unwrap_or_else(|e| e.into_inner()).retain(|id| *id != course_id);
    let mut finished = FINISHED.lock().unwrap_or_else(|e| e.into_inner());
    if !finished.contains(&course_id) {
        finished.push(course_id);
    }
}

fn finished(course_id: CourseId) -> bool {
    FINISHED.lock().unwrap_or_else(|e| e.into_inner()).contains(&course_id)
}

/// The database's path, for the job in the background. Set at startup.
static DB_PATH: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

pub fn set_db_path(path: &std::path::Path) {
    let _ = DB_PATH.set(path.to_path_buf());
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

// Spec: OFF-4
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

/// What a course still lacks in example sentences and audio.
struct Lacking {
    words: i64,
    /// Words without example sentences.
    examples: Vec<WordId>,
    clips: i64,
    /// Lines with no clip at normal speed, and with no slow one.
    normal: Vec<crate::speech::SpeechLine>,
    slow: Vec<crate::speech::SpeechLine>,
}

// Spec: OFF-3, OFF-15
fn lacking(conn: &mut Conn, course_id: CourseId) -> Result<Lacking> {
    let lesson_ids: Vec<LessonId> = lessons_of(conn, course_id)?.into_iter().map(|(id, _)| id).collect();
    let words: i64 = words::table.filter(words::lesson_id.eq_any(&lesson_ids)).count().get_result(conn)?;
    let examples = crate::examples::missing(conn, &lesson_ids)?;
    let normal = crate::speech::course_audio_in(conn, course_id)?;
    let slow = crate::speech::course_slow_audio_in(conn, course_id)?;
    let missing = |items: Vec<crate::speech::AudioItem>| -> Vec<crate::speech::SpeechLine> {
        items.into_iter().filter(|item| !item.stored).map(|item| crate::speech::SpeechLine { text: item.text, gender: item.gender }).collect()
    };
    Ok(Lacking {
        words,
        examples,
        clips: crate::convert::count(normal.len() + slow.len()),
        normal: missing(normal),
        slow: missing(slow),
    })
}

/// Whether a course lacks example sentences or audio there is a way to
/// fetch: audio always (with a key), example sentences only with a chat model.
fn needs_fetching(conn: &mut Conn, course_id: CourseId) -> bool {
    lacking(conn, course_id).is_ok_and(|l| {
        !l.normal.is_empty() || !l.slow.is_empty() || (!l.examples.is_empty() && crate::ai::partner_model(conn).is_ok())
    })
}

/// A course's name, as its page shows it: for the notification.
fn title(conn: &mut Conn, course_id: CourseId) -> String {
    courses::table.find(course_id).select(courses::title_pl).first(conn).unwrap_or_else(|_| "A course".to_string())
}

// Spec: OFF-5, OFF-9, OFF-10, OFF-16
pub fn status(conn: &mut Conn, course_id: CourseId) -> Result<OfflineStatus> {
    let enabled: Option<i64> = courses::table.find(course_id).select(courses::offline_at).first(conn)?;
    let lacking = lacking(conn, course_id)?;
    let pictures = pictures_of(conn, course_id)?;
    let online_lessons = lessons_of(conn, course_id)?
        .iter()
        .filter(|(_, lesson_type)| crate::lesson_types::get(lesson_type).is_ok_and(|t| !t.is_drill()))
        .count();
    let clips_stored = lacking.clips - crate::convert::count(lacking.normal.len() + lacking.slow.len());
    let examples_stored = lacking.words - crate::convert::count(lacking.examples.len());
    Ok(OfflineStatus {
        course_id,
        enabled: enabled.is_some(),
        clips: lacking.clips,
        clips_stored,
        examples: lacking.words,
        examples_stored,
        pictures: pictures.total,
        pictures_stored: pictures.stored,
        online_lessons: crate::convert::count(online_lessons),
        ready: clips_stored == lacking.clips && examples_stored == lacking.words && pictures.stored == pictures.total,
        working: is_working(course_id),
        can_download: crate::ai::api_key(conn).is_ok(),
        needs_model: !lacking.examples.is_empty() && crate::ai::partner_model(conn).is_err(),
    })
}

/// Marks a course to be kept offline, or takes the mark away. Taking it
/// away deletes nothing: what was downloaded stays (Settings → Audio clears
/// audio no course uses).
// Spec: OFF-1, OFF-11
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

/// How far one step of a download has come.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct Step {
    done: i64,
    failed: i64,
    total: i64,
}

impl Step {
    /// Items finished either way: what the progress bar shows.
    fn finished(&self) -> i64 {
        self.done + self.failed
    }
}

/// How far a course's download has come, step by step: example sentences,
/// audio, slow audio. A step's total is what it found missing when it
/// started (the audio's grows by what the example sentences say).
#[derive(Debug, Default)]
struct Tally {
    steps: std::sync::Mutex<[Step; 3]>,
}

const EXAMPLES: usize = 0;
const AUDIO: usize = 1;
const SLOW_AUDIO: usize = 2;

impl Tally {
    fn set(&self, step: usize, now: Step) {
        self.steps.lock().unwrap_or_else(|e| e.into_inner())[step] = now;
    }

    fn count(&self, step: usize, ok: bool) {
        let mut steps = self.steps.lock().unwrap_or_else(|e| e.into_inner());
        if ok {
            steps[step].done += 1;
        } else {
            steps[step].failed += 1;
        }
    }

    /// All steps together.
    fn sum(&self) -> Step {
        self.steps.lock().unwrap_or_else(|e| e.into_inner()).iter().fold(Step::default(), |sum, step| Step {
            done: sum.done + step.done,
            failed: sum.failed + step.failed,
            total: sum.total + step.total,
        })
    }
}

/// What the learner is told when a download ends: the course is ready, or
/// how much could not be downloaded. Nothing when neither (its pictures are
/// still being drawn).
fn result_line(title: &str, ready: bool, sum: Step) -> Option<String> {
    let missing = sum.total - sum.done;
    if ready {
        Some(format!("{title} is ready offline"))
    } else if missing > 0 {
        Some(format!("{title}: {missing} couldn't be downloaded. Try again from its page."))
    } else {
        None
    }
}

/// Fetches what a course lacks in example sentences and audio — its
/// examples first, as what they say is audio to fetch too — counting in
/// `tally` and telling `told` after each item. The database is never held
/// while waiting on the network.
async fn fetch(db: &Db, course_id: CourseId, tally: &Tally, told: &(dyn Fn(Step) + Sync)) {
    let audio_of = |lacking: &Lacking| {
        (Step { total: crate::convert::count(lacking.normal.len()), ..Step::default() }, Step {
            total: crate::convert::count(lacking.slow.len()),
            ..Step::default()
        })
    };
    let (words, can_write) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let Ok(lacking) = lacking(conn, course_id) else { return };
        let (audio, slow) = audio_of(&lacking);
        tally.set(AUDIO, audio);
        tally.set(SLOW_AUDIO, slow);
        (lacking.examples, crate::ai::partner_model(conn).is_ok())
    };
    if can_write {
        tally.set(EXAMPLES, Step { total: crate::convert::count(words.len()), ..Step::default() });
    }
    told(tally.sum());
    if !words.is_empty() && can_write {
        use futures_util::stream::{self, StreamExt};
        stream::iter(words)
            .for_each_concurrent(PARALLEL_EXAMPLES, |word_id| async move {
                let made = crate::examples::make(db, word_id).await;
                if let Err(error) = &made {
                    crate::diagnostics::note("background", format!("Example sentences for {word_id} failed: {error}"));
                }
                tally.count(EXAMPLES, matches!(made, Ok(Some(_))));
                told(tally.sum());
            })
            .await;
    }
    // Read on its own line: the database is not held while downloading.
    let Ok(lacking) = lacking(&mut db.lock(), course_id) else { return };
    let (audio, slow) = audio_of(&lacking);
    tally.set(AUDIO, audio);
    tally.set(SLOW_AUDIO, slow);
    told(tally.sum());
    for (step, lines, slow) in [(AUDIO, lacking.normal, false), (SLOW_AUDIO, lacking.slow, true)] {
        crate::speech::fill_with(db, lines, false, slow, |progress| {
            tally.set(step, Step { done: progress.done, failed: progress.failed, total: progress.total });
            told(tally.sum());
        })
        .await;
    }
}

/// On Android, makes sure the job in the background (background.rs) would
/// carry on with this download if the app were stopped before it ends.
/// Tried a few times: the plugin is ready only once the screens are.
fn keep_going_without_the_app(app: &AppHandle) {
    if !cfg!(target_os = "android") {
        return;
    }
    let Some(path) = DB_PATH.get().cloned() else { return };
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut tries = 0;
        loop {
            match tauri_plugin_sync_background::schedule_offline(&app, &path.to_string_lossy()) {
                Ok(()) => return,
                Err(error) if tries >= 10 => {
                    crate::diagnostics::note("background", format!("Could not make sure a course's download finishes without the app: {error}"));
                    return;
                }
                Err(_) => tries += 1,
            }
            std::thread::sleep(std::time::Duration::from_secs(3));
        }
    });
}

/// Downloads what a course lacks, in the background, unless that is going
/// on already: its example sentences, then its audio (the examples' too),
/// at normal speed and slowly. With `everything` (the learner asked) its
/// pictures too, those that failed before included. Without a key there is
/// nothing to download with, and nothing happens.
// Spec: OFF-2, OFF-8, OFF-13, OFF-14
pub fn download_in_background(app: &AppHandle, course_id: CourseId, everything: bool) {
    let db = app.state::<Db>();
    let (needed, pictures, title) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        if crate::ai::api_key(conn).is_err() {
            return;
        }
        let needed = needs_fetching(conn, course_id);
        let pictures = if everything { pictures_of(conn, course_id).unwrap_or_default() } else { Pictures::default() };
        // Panels that failed are set to be drawn again: the learner asked.
        for lesson_id in &pictures.stories {
            let _ = crate::stories::redraw(conn, *lesson_id, None);
        }
        (needed, pictures, title(conn, course_id))
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
    if !needed {
        let _ = app.emit("offline://changed", OfflineChanged { course_id });
        return;
    }
    if !claim(course_id) {
        return;
    }
    let busy = crate::work::begin_counted(app, &title);
    keep_going_without_the_app(app);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = app.emit("offline://changed", OfflineChanged { course_id });
        let db = app.state::<Db>();
        let db: &Db = &db;
        let tally = Tally::default();
        fetch(db, course_id, &tally, &|step| busy.progress(step.finished(), step.total)).await;
        release(course_id);
        let ready = status(&mut db.lock(), course_id).is_ok_and(|s| s.ready);
        busy.finish(result_line(&title, ready, tally.sum()));
        let _ = app.emit("offline://changed", OfflineChanged { course_id });
    });
}

/// Fetches the example sentences and audio the courses kept offline have
/// gained since they were last complete — at startup, and after the tutor
/// adds to a course.
// Spec: OFF-7
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_download_counts_its_steps_together() {
        let tally = Tally::default();
        tally.set(EXAMPLES, Step { total: 4, ..Step::default() });
        tally.set(AUDIO, Step { total: 10, ..Step::default() });
        tally.count(EXAMPLES, true);
        tally.count(EXAMPLES, false);
        // The audio step says where it is itself.
        tally.set(AUDIO, Step { done: 6, failed: 1, total: 12 });
        let sum = tally.sum();
        assert_eq!(sum, Step { done: 7, failed: 2, total: 16 });
        assert_eq!(sum.finished(), 9);
    }

    #[test]
    fn a_download_ends_saying_how_it_went() {
        let all = Step { done: 5, failed: 0, total: 5 };
        assert_eq!(result_line("Pierwsze kroki", true, all).as_deref(), Some("Pierwsze kroki is ready offline"));
        let some = Step { done: 3, failed: 1, total: 5 };
        assert_eq!(
            result_line("Pierwsze kroki", false, some).as_deref(),
            Some("Pierwsze kroki: 2 couldn't be downloaded. Try again from its page.")
        );
        // Everything fetched, pictures still being drawn: nothing to say yet.
        assert_eq!(result_line("Pierwsze kroki", false, all), None);
    }
}

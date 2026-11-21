//! Quick games: a minute's play over words the learner has already met, from
//! Home, outside the daily five. *Match pairs* (eight Polish–English pairs,
//! against the clock), *Flip and match* (memory cards: a word and its
//! picture where it has one, else its English; counted in moves),
//! *Dictation* (five sentences the learner has met, said aloud, written
//! down) and *Sound pairs* (the built-in pairs of sounds.rs: which word was
//! said?) and *Say it* (five sentences met, said aloud: speaking.rs). The
//! last three are scored in right answers.
//!
//! They pay a little XP — a set amount per game finished, up to a cap a day,
//! so they never stand in for lessons — written to the XP log under a run of
//! their own (`game:…`), so the day's XP screen names them.
//!
//! They leave the schedule alone: no review is written. Matching from a
//! handful of cards you can see is recognition helped by elimination, not
//! recall; counting it as a review would push the words' next check-in out
//! on evidence that doesn't hold. The games are play over what the lessons
//! taught, and the lessons keep the say over when a word comes back.
//!
//! Best results are settings (synced), one per game: a record set on the
//! phone is the one to beat on the computer.

use diesel::prelude::*;
use rand::seq::SliceRandom;
use serde::Serialize;
use tauri::State;

use crate::db::{self, Conn, Db};
use crate::error::{Error, Result};
use crate::models::Pair;
use crate::schema::{reviews, sentences, words, xp_log};
use crate::session::{pick_pairs, WordRow};
use crate::sounds::SoundPair;
#[allow(unused_imports)]
use crate::ids::*;

/// The games, by the name the screens use.
const GAMES: &[&str] = &["match", "flip", "dictation", "sounds", "say"];
/// Sentences in a game of Dictation…
// Spec: GAME-3
const DICTATION_SENTENCES: usize = 5;
/// …and at least this many met to play it.
pub const MIN_SENTENCES: usize = 3;
/// Pairs in a game of Sound pairs.
// Spec: GAME-3
const SOUND_ROUNDS: usize = 8;
/// Pairs in a game of Match pairs.
// Spec: GAME-2
const MATCH_PAIRS: usize = 8;
/// Pairs in a game of Flip and match (twelve cards).
// Spec: GAME-2
const FLIP_PAIRS: usize = 6;
/// Fewer met words than this and there is no game to play.
pub const MIN_PAIRS: usize = 4;
/// XP for a game finished…
// Spec: GAME-4
const GAME_XP: i64 = 10;
/// …up to this much a day, over every device: a few games, not a lesson's worth.
// Spec: GAME-4
pub const GAME_XP_PER_DAY: i64 = 30;
/// The XP log's run key of a game starts with this (xplog.rs names the run).
pub const RUN_PREFIX: &str = "game:";
/// Faster than this is no real game (a stale tap, a clock gone wrong).
const MIN_MS: i64 = 1500;

fn title(game: &str) -> &'static str {
    match game {
        "match" => "Match pairs",
        "dictation" => "Dictation",
        "sounds" => "Sound pairs",
        "say" => "Say it",
        _ => "Flip and match",
    }
}

/// Games scored in right answers (out of the rounds), with no best kept.
fn scored(game: &str) -> bool {
    matches!(game, "dictation" | "sounds" | "say")
}

fn check(game: &str) -> Result<()> {
    if GAMES.contains(&game) {
        Ok(())
    } else {
        Err(Error::msg(format!("There is no quick game called '{game}'.")))
    }
}

/// The words the learner has met (has a review of), each Polish word once.
fn met_words(conn: &mut Conn) -> Result<Vec<WordRow>> {
    let ids: Vec<WordId> = reviews::table
        .filter(reviews::kind.eq("word"))
        .select(reviews::item_id)
        .load::<String>(conn)?
        .iter()
        .filter_map(|id| WordId::parse(id).ok())
        .collect();
    let rows: Vec<(WordId, String, String, String, Option<FileId>)> = words::table
        .filter(words::id.eq_any(ids))
        .order(words::id)
        .select((words::id, words::pl, words::en, words::pos, words::image_file_id))
        .load(conn)?;
    let mut seen = std::collections::HashSet::new();
    Ok(rows
        .into_iter()
        .filter(|(_, pl, ..)| seen.insert(pl.to_lowercase()))
        .map(|(id, pl, meaning, pos, image)| WordRow { id, pl, meaning, pos, reps: 0, image, near: None })
        .collect())
}

/// A sentence to write down in Dictation.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DictationItem {
    pub id: SentenceId,
    pub pl: String,
    pub meaning: String,
}

/// The sentences the learner has met, each once.
fn met_sentences(conn: &mut Conn) -> Result<Vec<DictationItem>> {
    let ids: Vec<SentenceId> = reviews::table
        .filter(reviews::kind.eq("sentence"))
        .select(reviews::item_id)
        .load::<String>(conn)?
        .iter()
        .filter_map(|id| SentenceId::parse(id).ok())
        .collect();
    let rows: Vec<(SentenceId, String, String)> = sentences::table
        .filter(sentences::id.eq_any(ids))
        .order(sentences::id)
        .select((sentences::id, sentences::pl, sentences::en))
        .load(conn)?;
    let mut seen = std::collections::HashSet::new();
    Ok(rows
        .into_iter()
        .filter(|(_, pl, _)| seen.insert(pl.to_lowercase()))
        .map(|(id, pl, meaning)| DictationItem { id, pl, meaning })
        .collect())
}

/// A round of Sound pairs: the pair, and which of it is said.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SoundRound {
    #[serde(flatten)]
    pub pair: SoundPair,
    /// The word said: `a` or `b`.
    pub said: String,
}

/// XP the quick games earned on a day.
fn xp_on(conn: &mut Conn, day: i64) -> Result<i64> {
    Ok(xp_log::table
        .filter(xp_log::day.eq(day))
        .filter(xp_log::run.like(format!("{RUN_PREFIX}%")))
        .select(xp_log::xp)
        .load::<i64>(conn)?
        .into_iter()
        .sum())
}

fn best(conn: &mut Conn, key: &str) -> Option<i64> {
    db::get_setting(conn, key).ok().flatten().and_then(|v| v.parse().ok()).filter(|v: &i64| *v > 0)
}

/// What Home shows of the games: whether there are enough words, the records,
/// and today's XP from them.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QuickGames {
    /// Words met so far: the games need at least `minWords`.
    pub words_met: i64,
    pub min_words: i64,
    /// Sentences met so far: Dictation needs at least `minSentences`.
    pub sentences_met: i64,
    pub min_sentences: i64,
    /// Match pairs: the fastest game, in milliseconds.
    pub match_best_ms: Option<i64>,
    /// Flip and match: the fewest moves, and that game's time.
    pub flip_best_moves: Option<i64>,
    pub flip_best_ms: Option<i64>,
    pub xp_today: i64,
    pub xp_per_day: i64,
    pub xp_per_game: i64,
}

pub fn overview(conn: &mut Conn) -> Result<QuickGames> {
    let today = db::today(conn);
    let words_met = crate::convert::count(met_words(conn)?.len());
    Ok(QuickGames {
        words_met,
        min_words: crate::convert::count(MIN_PAIRS),
        sentences_met: crate::convert::count(met_sentences(conn)?.len()),
        min_sentences: crate::convert::count(MIN_SENTENCES),
        match_best_ms: best(conn, "game_best_match_ms"),
        flip_best_moves: best(conn, "game_best_flip_moves"),
        flip_best_ms: best(conn, "game_best_flip_ms"),
        xp_today: xp_on(conn, today)?,
        xp_per_day: GAME_XP_PER_DAY,
        xp_per_game: GAME_XP,
    })
}

/// One game's cards.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QuickGame {
    pub game: String,
    /// Match pairs, Flip and match: the cards.
    pub pairs: Vec<Pair>,
    /// Dictation, Say it: the sentences to write down or say.
    pub sentences: Vec<DictationItem>,
    /// Sound pairs: the rounds.
    pub sounds: Vec<SoundRound>,
}

/// Deals a game from the words met: random ones for Match pairs; for Flip
/// and match, pictured words first (their card is the picture), then others.
/// Dictation deals sentences met; Sound pairs, built-in pairs, one of each
/// contrast before any comes twice.
pub fn deal(conn: &mut Conn, game: &str, rng: &mut impl rand::Rng) -> Result<QuickGame> {
    check(game)?;
    if game == "dictation" || game == "say" {
        let mut met = met_sentences(conn)?;
        if met.len() < MIN_SENTENCES {
            return Err(Error::msg(format!("Meet a few sentences in your lessons first: {} uses sentences you know.", title(game))));
        }
        met.shuffle(rng);
        met.truncate(DICTATION_SENTENCES);
        return Ok(QuickGame { game: game.into(), pairs: vec![], sentences: met, sounds: vec![] });
    }
    if game == "sounds" {
        let mut pairs = crate::sounds::built_in();
        pairs.shuffle(rng);
        // One of each contrast first, then the rest.
        let mut seen = std::collections::HashSet::new();
        let (mut first, rest): (Vec<SoundPair>, Vec<SoundPair>) = pairs.into_iter().partition(|p| seen.insert(p.label.clone()));
        first.extend(rest);
        first.truncate(SOUND_ROUNDS);
        let sounds = first
            .into_iter()
            .map(|pair| {
                let said = if rng.random_bool(0.5) { pair.a.clone() } else { pair.b.clone() };
                SoundRound { pair, said }
            })
            .collect();
        return Ok(QuickGame { game: game.into(), pairs: vec![], sentences: vec![], sounds });
    }
    let mut met = met_words(conn)?;
    met.shuffle(rng);
    let pairs = if game == "match" {
        let pool: Vec<&WordRow> = met.iter().collect();
        pick_pairs(&pool, MATCH_PAIRS, false)
    } else {
        let pool: Vec<&WordRow> =
            met.iter().filter(|w| w.image.is_some()).chain(met.iter().filter(|w| w.image.is_none())).collect();
        let mut pairs = pick_pairs(&pool, FLIP_PAIRS, true);
        pairs.shuffle(rng);
        pairs
    };
    if pairs.len() < MIN_PAIRS {
        return Err(Error::msg("Meet a few more words in your lessons first: the games use words you know."));
    }
    Ok(QuickGame { game: game.into(), pairs, sentences: vec![], sounds: vec![] })
}

/// What finishing a game brought.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GameFinished {
    pub xp_gained: i64,
    /// Better than the best before it (not the first game: that only sets one).
    pub record: bool,
    /// The best now, and the one before this game.
    pub best_ms: Option<i64>,
    pub best_moves: Option<i64>,
    pub previous_ms: Option<i64>,
    pub previous_moves: Option<i64>,
    pub xp_today: i64,
    pub xp_per_day: i64,
    pub total_xp: i64,
    pub level: i64,
}

/// "0:42" for 42 seconds.
fn clock(ms: i64) -> String {
    let seconds = ms / 1000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// Records a finished game: its XP (within the day's cap) and, if it beat
/// the best, the new best. `pairs`: the pairs, sentences or rounds played;
/// `moves`: turns in Flip and match, wrong taps in Match pairs, misses in
/// Dictation and Sound pairs (which keep no best: a score out of a handful
/// is no record to chase).
pub fn finish(conn: &mut Conn, game: &str, pairs: i64, ms: i64, moves: i64) -> Result<GameFinished> {
    check(game)?;
    let ms = ms.max(MIN_MS);
    let moves = moves.max(0);
    let today = db::today(conn);
    conn.transaction::<_, Error, _>(|conn| {
        crate::learnlog::note(
            conn,
            "game_finished",
            "",
            "",
            None,
            "",
            crate::learnlog::EventDetails { game: Some(game.into()), ms: Some(ms), answered: Some(pairs), ..Default::default() },
            &crate::learnlog::PracticeContext { source: "game".into(), course_id: None },
        )?;
        let earned = xp_on(conn, today)?;
        let xp = GAME_XP.min(GAME_XP_PER_DAY - earned).max(0);
        let (previous_ms, previous_moves, record) = if scored(game) {
            (None, None, false)
        } else if game == "match" {
            let before = best(conn, "game_best_match_ms");
            let better = before.map_or(true, |b| ms < b);
            if better {
                db::set_setting(conn, "game_best_match_ms", &ms.to_string())?;
            }
            (before, None, better && before.is_some())
        } else {
            let (before_moves, before_ms) = (best(conn, "game_best_flip_moves"), best(conn, "game_best_flip_ms"));
            let better = match (before_moves, before_ms) {
                (Some(m), Some(t)) => moves < m || (moves == m && ms < t),
                (Some(m), None) => moves < m,
                _ => true,
            };
            if better {
                db::set_setting(conn, "game_best_flip_moves", &moves.to_string())?;
                db::set_setting(conn, "game_best_flip_ms", &ms.to_string())?;
            }
            (before_ms, before_moves, better && before_moves.is_some())
        };
        let label = if scored(game) {
            let what = if game == "sounds" { "pairs" } else { "sentences" };
            format!("{}: {} of {pairs} {what} right", title(game), (pairs - moves).max(0))
        } else if game == "match" {
            format!("{}: {pairs} pairs in {}", title(game), clock(ms))
        } else {
            format!("{}: {pairs} pairs in {moves} moves", title(game))
        };
        let note = match (xp, record) {
            (0, _) => format!("quick games: today's {GAME_XP_PER_DAY} XP already earned"),
            (_, true) => "a new best".to_string(),
            _ => String::new(),
        };
        if xp > 0 {
            crate::stats::add_xp(conn, today, xp)?;
        }
        // Each game its own run on the day's XP screen (a fresh id: two games
        // can end within a second).
        let run = format!("{RUN_PREFIX}{game}:{}", XpEntryId::new());
        crate::xplog::write(conn, today, &run, None, &label, xp, &note)?;
        // A game counts toward the day like a drill: on a day with no
        // lessons to give, or once they are done, it can be what does it.
        crate::stats::mark_practised(conn, today)?;
        let total_xp = crate::stats::total_xp(conn)?;
        Ok(GameFinished {
            xp_gained: xp,
            record,
            best_ms: match game {
                "match" => best(conn, "game_best_match_ms"),
                "flip" => best(conn, "game_best_flip_ms"),
                _ => None,
            },
            best_moves: if game == "flip" { best(conn, "game_best_flip_moves") } else { None },
            previous_ms,
            previous_moves,
            xp_today: earned + xp,
            xp_per_day: GAME_XP_PER_DAY,
            total_xp,
            level: crate::stats::level_from_xp(total_xp),
        })
    })
}

/// One answer in a quick game that grades its answers (Dictation, Sound
/// pairs, Say it): graded here, like a lesson's, and written to the
/// learning log with its mistake (no XP or schedule: the game's XP comes
/// when it ends).
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameAnswer {
    pub game: String,
    /// The sentence (Dictation, Say it) or the built-in sound pair.
    #[serde(default)]
    pub item_id: Option<String>,
    pub expected: String,
    /// What was typed, picked or heard (a transcript).
    pub given: String,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub run: String,
    #[serde(default)]
    pub signals: Option<crate::learnlog::Signals>,
    /// Where it is played from: a quick game, unless the screen says else.
    #[serde(default)]
    pub context: Option<crate::learnlog::PracticeContext>,
    /// The round's place in the game, and what else it showed.
    #[serde(default)]
    pub card: Option<crate::learnlog::CardShown>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameAnswered {
    pub correct: bool,
    pub words: Vec<crate::dictation::DiffWord>,
    pub tips: Vec<crate::sounds::SoundTip>,
}

pub fn answer(conn: &mut Conn, a: GameAnswer) -> Result<GameAnswered> {
    let (correct, words, tips) = match a.game.as_str() {
        "dictation" => {
            let checked = crate::dictation::check(&a.expected, &a.given);
            (checked.correct, checked.words, vec![])
        }
        "sounds" => (a.expected.trim().to_lowercase() == a.given.trim().to_lowercase(), vec![], vec![]),
        "say" => {
            let spoken = crate::speaking::check(&a.expected, &a.given);
            (spoken.correct, spoken.words, spoken.tips)
        }
        other => return Err(Error::msg(format!("The game '{other}' does not grade answers."))),
    };
    let exercise = format!("{RUN_PREFIX}{}", a.game);
    let item_id = a.item_id.unwrap_or_default();
    let item_kind = match crate::ids::kind_of(&item_id) {
        Some("sentence") => "sentence",
        Some("sound pair") => "sound",
        _ => "set",
    };
    let mistake = crate::mistakes::classify(&crate::mistakes::Graded {
        exercise: &exercise,
        expected: &a.expected,
        given: &a.given,
        correct,
        diff: &words,
        options: &a.options,
        ..Default::default()
    });
    let signals = a.signals.clone().unwrap_or_default();
    let context = crate::learnlog::resolve(conn, a.context.as_ref(), None, "game")?;
    let prompt = if a.game == "sounds" { "Which did you hear?" } else { "" };
    conn.transaction::<_, Error, _>(|conn| {
        crate::learnlog::write_attempt(
            conn,
            &crate::learnlog::NewAttempt {
                item_kind,
                item_id: if item_kind == "set" { "" } else { &item_id },
                lesson_id: None,
                exercise: &exercise,
                run: &a.run,
                correct,
                given: &a.given,
                expected: &a.expected,
                mistake: &mistake,
                signals: &signals,
                options: &a.options,
                grammar: vec![],
                note: String::new(),
                context: &context,
                prompt,
                card: a.card.as_ref(),
                pl: "",
                checkin_id: None,
                verdict: None,
            },
        )
    })?;
    Ok(GameAnswered { correct, words, tips })
}

#[tauri::command]
pub async fn quick_game_answer(db: State<'_, Db>, answer: GameAnswer) -> Result<GameAnswered> {
    self::answer(&mut db.lock(), answer)
}

#[tauri::command]
pub async fn quick_games(db: State<'_, Db>) -> Result<QuickGames> {
    overview(&mut db.lock())
}

#[tauri::command]
pub async fn quick_game_deal(db: State<'_, Db>, game: String) -> Result<QuickGame> {
    deal(&mut db.lock(), &game, &mut rand::rng())
}

#[tauri::command]
pub async fn quick_game_finish(db: State<'_, Db>, game: String, pairs: i64, ms: i64, moves: i64) -> Result<GameFinished> {
    finish(&mut db.lock(), &game, pairs, ms, moves)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clock_reads_minutes_and_seconds() {
        assert_eq!(clock(42_300), "0:42");
        assert_eq!(clock(61_000), "1:01");
    }
}

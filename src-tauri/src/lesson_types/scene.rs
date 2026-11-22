//! Scenes: one illustrated scene full of things to find. Explore it (tap a
//! thing to hear it), find a thing by its Polish name, name a thing that
//! lights up, and — when the tutor wrote some — say where things are ("Kot
//! jest pod stołem"). The things are the lesson's own words; the picture
//! and where each thing is in it are made in the background (crate::scenes).
//! Until it is ready, the words are practised as a plain word drill.

use rand::seq::{IndexedRandom, SliceRandom};
use rand::Rng as _;

use super::{LessonType, PathGroup, Run};
use crate::db::Conn;
use crate::error::Result;
use crate::models::Exercise;
use crate::scenes::{SceneCard, Statement};
use crate::session::{self, Built, Drill, WordRow};

pub const TYPE: LessonType = LessonType {
    id: "scene",
    name: "Scene",
    icon: "scene",
    description: "One illustrated scene — a market stall, a kitchen, a station — with its things to explore: tap a thing to hear its name, find a thing in the picture by its Polish name, name a thing that lights up, and say where things are.",
    tutor_hint: "For a place full of everyday things (create_scene_lesson): 6–10 things you can point at, nouns only, the course's own words where they fit. Add “Where is it?” sentences (pod, na, nad, obok, przy, w) when the learner is ready for prepositions with cases.",
    run: Run::Exercises(build),
    path: PathGroup::Words,
    // Made with its own tutor tool (create_scene_lesson): a scene and its things.
    takes_items: false,
    unaided_to_master: false,
    sections: &["scene", "words", "where"],
    start_label: "Start",
    again_label: "Practise again",
};

/// A drill's word, and whether it is new.
type Word = (WordRow, bool);

/// "Where is it?" cards a session asks at most.
const WHERE_CARDS: usize = 3;
/// A known word is named by typing it (harder lessons) once it has come
/// back this many times.
const TYPE_AFTER: i64 = 2;

fn build(conn: &mut Conn, drill: &Drill) -> Result<Built> {
    let rng = &mut rand::rng();
    let Some((card, statements)) = crate::scenes::card(conn, drill.lesson.id)? else {
        // Still being drawn (or failed): the words, as a word drill.
        return Ok(Built { exercises: plain(drill.words.iter().collect(), drill, rng), ..Default::default() });
    };
    let (spotted, unspotted): (Vec<&Word>, Vec<&Word>) =
        drill.words.iter().partition(|(w, _)| card.spots.iter().any(|s| s.word_id == w.id));
    let mut exercises = Vec::new();
    if spotted.iter().any(|(_, is_new)| *is_new) {
        exercises.push(explore(drill, &card));
    }
    // A name is typed only where the level allows typing a word.
    // Spec: DRILL-14
    let typed = drill.lesson.complexity != "low" && crate::difficulty::allows(drill.level, "type-word");
    let mut finds = Vec::new();
    let mut names = Vec::new();
    for (w, is_new) in &spotted {
        if *is_new {
            finds.push(find(w, true, &card));
            names.push(name(w, false, &card, drill, rng));
        } else if w.reps % 2 == 0 {
            finds.push(find(w, false, &card));
        } else {
            names.push(name(w, typed && w.reps >= TYPE_AFTER, &card, drill, rng));
        }
    }
    finds.shuffle(rng);
    names.shuffle(rng);
    exercises.extend(finds);
    exercises.extend(names);
    exercises.extend(plain(unspotted, drill, rng));
    let mut statements: Vec<&Statement> = statements.iter().collect();
    statements.shuffle(rng);
    for (n, statement) in statements.into_iter().take(WHERE_CARDS).enumerate() {
        let truth = rng.random_bool(0.5);
        exercises.push(where_is(statement, n, truth, drill.lesson.id, &card, rng));
    }
    Ok(Built { exercises, ..Default::default() })
}

/// Words without a place in the picture: new ones met and recognised,
/// known ones practised as in a word drill.
fn plain(words: Vec<&Word>, drill: &Drill, rng: &mut impl rand::Rng) -> Vec<Exercise> {
    let mut exercises = Vec::new();
    for (w, is_new) in words {
        if *is_new {
            exercises.push(session::word_intro(w));
            exercises.push(session::recognise(w, &drill.pool, rng));
        } else {
            exercises.push(session::practise_word(w, drill, rng));
        }
    }
    exercises
}

/// The scene with the given things pointed out.
fn lit(card: &SceneCard, lit: &[crate::ids::WordId]) -> SceneCard {
    let mut card = card.clone();
    for spot in &mut card.spots {
        spot.lit = lit.contains(&spot.word_id);
    }
    card
}

/// The whole scene, to look around: tap a thing to hear its name.
fn explore(drill: &Drill, card: &SceneCard) -> Exercise {
    let id = drill.lesson.id;
    Exercise {
        key: format!("b{id}-explore"),
        kind: "sceneExplore".into(),
        item_kind: "set".into(),
        item_id: id.to_string(),
        is_new: true,
        prompt: drill.lesson.title.clone(),
        scene: Some(card.clone()),
        ..Exercise::default()
    }
}

/// "Gdzie jest zegar?" — tap it in the picture.
fn find(w: &WordRow, is_new: bool, card: &SceneCard) -> Exercise {
    let verb = if w.pos == "n-pl" { "są" } else { "jest" };
    Exercise {
        key: format!("w{}-find", w.id),
        kind: "sceneFind".into(),
        item_kind: "word".into(),
        item_id: w.id.to_string(),
        is_new,
        prompt: format!("Gdzie {verb} {}?", w.pl),
        answer: w.pl.clone(),
        pl: w.pl.clone(),
        meaning: w.meaning.clone(),
        scene: Some(card.clone()),
        ..Exercise::default()
    }
}

/// A thing lights up: pick its name from the scene's others, or type it.
fn name(w: &WordRow, typed: bool, card: &SceneCard, drill: &Drill, rng: &mut impl rand::Rng) -> Exercise {
    let mut options = Vec::new();
    if !typed {
        let mut others: Vec<String> = card.spots.iter().filter(|s| s.pl != w.pl).map(|s| s.pl.clone()).collect();
        others.shuffle(rng);
        others.truncate(3);
        // A small scene borrows decoys from the course.
        let mut pool: Vec<String> = drill.pool.polish.iter().filter(|p| **p != w.pl && !others.contains(p)).cloned().collect();
        pool.shuffle(rng);
        others.extend(pool.into_iter().take(3 - others.len()));
        options = others;
        options.push(w.pl.clone());
        options.shuffle(rng);
    }
    Exercise {
        key: format!("w{}-name", w.id),
        kind: if typed { "sceneType" } else { "sceneName" }.into(),
        item_kind: "word".into(),
        item_id: w.id.to_string(),
        is_new: false,
        prompt: "Co to jest?".into(),
        hint: session::pretty_pos(&w.pos),
        answer: w.pl.clone(),
        options,
        pl: w.pl.clone(),
        meaning: w.meaning.clone(),
        scene: Some(lit(card, &[w.id])),
        ..Exercise::default()
    }
}

/// The two Polish words for true and false, the options of a true-or-false card.
pub const TRUE: &str = "Prawda";
pub const FALSE: &str = "Fałsz";

/// "Where is it?": a sentence about the scene, true or not (`truth`), or with
/// the place left out to choose. The two things it is about are pointed out.
fn where_is(s: &Statement, n: usize, truth: bool, lesson: crate::ids::LessonId, card: &SceneCard, rng: &mut impl rand::Rng) -> Exercise {
    let base = Exercise {
        key: format!("b{lesson}-where{n}"),
        kind: "sceneWhere".into(),
        item_kind: "set".into(),
        item_id: lesson.to_string(),
        pl: s.pl.clone(),
        meaning: s.en.clone(),
        scene: Some(lit(card, &[s.subject, s.object])),
        ..Exercise::default()
    };
    let wrong = s.wrong.choose(rng).cloned().unwrap_or_default();
    // Every other card asks for the place itself.
    if n % 2 == 1 && !wrong.is_empty() {
        let mut options: Vec<String> = s.wrong.clone();
        options.push(s.phrase.clone());
        options.shuffle(rng);
        return Exercise {
            prompt: s.pl.replacen(&s.phrase, "…", 1),
            hint: "Which fits the picture?".into(),
            explanation: crate::scenes::explain_place(s, &s.phrase),
            answer: s.phrase.clone(),
            options,
            ..base
        };
    }
    let shown = if truth || wrong.is_empty() { s.pl.clone() } else { s.pl.replacen(&s.phrase, &wrong, 1) };
    Exercise {
        prompt: shown,
        hint: "True or false? Look at the picture.".into(),
        answer: if truth || wrong.is_empty() { TRUE } else { FALSE }.into(),
        options: vec![TRUE.into(), FALSE.into()],
        explanation: crate::scenes::explain_place(s, if truth || wrong.is_empty() { &s.phrase } else { &wrong }),
        ..base
    }
}

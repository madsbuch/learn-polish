//! Stories: a short illustrated story read panel by panel — a picture, one
//! to three Polish sentences under it (each to hear, normally or slowly, in
//! the voice of whoever says it; each word tappable for its meaning) — with
//! a quick question after each panel: who, what or where; which picture
//! shows a sentence; or which of two things happened first. Its last page
//! lists the words met; then the story's words (and the sentences the tutor
//! marked) are practised like any drill's. The story itself lives in
//! crate::stories; until it exists (a lesson made some other way) the
//! lesson is a plain word and sentence drill.

use rand::seq::{IndexedRandom, SliceRandom};

use super::{LessonType, PathGroup, Run};
use crate::db::Conn;
use crate::error::Result;
use crate::models::Exercise;
use crate::session::{self, Built, Drill};
use crate::stories::{Question, Story, StoryCard, StoryPanel, StoryWord};

pub const TYPE: LessonType = LessonType {
    id: "story",
    name: "Story",
    icon: "story",
    description: "A short illustrated story told panel by panel — a picture and one to three Polish sentences under it, to hear and to tap for meanings — with a quick question after each panel, then its new words practised. It stays readable as a picture book on its page.",
    tutor_hint: "For reading and listening in context (create_story_lesson): 4–8 panels at the lesson's level, built from the course's words, with 1–4 characters each described by how they look so every picture shows them the same.",
    run: Run::Exercises(build),
    path: PathGroup::Sentences,
    // Made with its own tutor tool (create_story_lesson): a story and its words.
    takes_items: false,
    unaided_to_master: false,
    sections: &["story", "words", "sentences"],
    start_label: "Read the story",
    again_label: "Read it again",
};

fn build(conn: &mut Conn, drill: &Drill) -> Result<Built> {
    let rng = &mut rand::rng();
    let Some(story) = crate::stories::load(conn, drill.lesson.id)? else {
        // No story (a lesson made as a plain drill): its words and sentences.
        return Ok(Built { exercises: practise(drill, true, rng), ..Default::default() });
    };
    let glossary = crate::stories::glossary(conn, drill.lesson.id, &story)?;
    let mut exercises = tell(drill, &story, &glossary, rng);
    exercises.extend(practise(drill, false, rng));
    Ok(Built { exercises, ..Default::default() })
}

/// The story itself: each panel, then its question, and the last page.
pub(crate) fn tell(drill: &Drill, story: &Story, glossary: &[crate::singalong::GlossWord], rng: &mut impl rand::Rng) -> Vec<Exercise> {
    let title = drill.lesson.title.clone();
    let total = crate::convert::count(story.panels.len());
    let polish = drill.lesson.complexity != "low";
    // One question asks which picture shows a sentence, and one which of two
    // things came first — each after a panel past the first, when there is
    // something to compare; the others are the tutor's own.
    let later: Vec<usize> = (1..story.panels.len()).collect();
    let pictured = later.iter().copied().filter(|i| story.panels[..=*i].iter().filter(|(p, _)| p.image.is_some()).count() >= 2).collect::<Vec<_>>();
    let picture_at = pictured.choose(rng).copied();
    let ordered: Vec<usize> = later.iter().copied().filter(|i| Some(*i) != picture_at).collect();
    let order_at = ordered.choose(rng).copied();
    let mut exercises = Vec::new();
    for (i, (panel, question)) in story.panels.iter().enumerate() {
        exercises.push(Exercise {
            key: format!("p{}-read", panel.id),
            kind: "storyPanel".into(),
            item_kind: "set".into(),
            item_id: drill.lesson.id.to_string(),
            is_new: true,
            prompt: title.clone(),
            pl: panel.lines.first().map(|l| l.pl.clone()).unwrap_or_default(),
            story: Some(StoryCard {
                title: title.clone(),
                panel: Some(panel.clone()),
                total,
                glossary: crate::stories::glossary_for(glossary, panel),
                ..StoryCard::default()
            }),
            ..Exercise::default()
        });
        let shown = &story.panels[..=i];
        let asked = if Some(i) == picture_at {
            which_picture(drill, panel, shown, total, rng)
        } else if Some(i) == order_at {
            which_first(drill, panel, shown, total, polish, rng)
        } else {
            None
        };
        exercises.push(asked.unwrap_or_else(|| choice(drill, panel, question, total, rng)));
    }
    let words = drill
        .words
        .iter()
        .map(|(w, is_new)| StoryWord { word_id: w.id, pl: w.pl.clone(), en: w.meaning.clone(), new: *is_new })
        .collect();
    exercises.push(Exercise {
        key: format!("b{}-story-end", drill.lesson.id),
        kind: "storyEnd".into(),
        item_kind: "set".into(),
        item_id: drill.lesson.id.to_string(),
        prompt: title.clone(),
        story: Some(StoryCard { title, total, words, ..StoryCard::default() }),
        ..Exercise::default()
    });
    exercises
}

/// A question card about a panel (not scheduled: the story's words are).
fn question_card(drill: &Drill, panel: &StoryPanel, total: i64, ask: &str) -> Exercise {
    Exercise {
        key: format!("p{}-{ask}", panel.id),
        kind: "storyQuestion".into(),
        item_kind: "set".into(),
        item_id: drill.lesson.id.to_string(),
        story: Some(StoryCard { title: drill.lesson.title.clone(), total, ask: ask.into(), ..StoryCard::default() }),
        ..Exercise::default()
    }
}

/// The tutor's question: who, what, where — its answer among the wrong ones.
fn choice(drill: &Drill, panel: &StoryPanel, q: &Question, total: i64, rng: &mut impl rand::Rng) -> Exercise {
    let mut options: Vec<String> = q.wrong.clone();
    options.push(q.answer.clone());
    options.shuffle(rng);
    let mut card = question_card(drill, panel, total, "choice");
    // The panel's picture is shown with the question (not its sentences).
    if let Some(story) = card.story.as_mut() {
        story.panel = Some(StoryPanel { lines: vec![], ..panel.clone() });
    }
    Exercise { prompt: q.prompt.clone(), answer: q.answer.clone(), options, ..card }
}

/// "Which picture shows this?": a sentence from a page read so far, and two
/// of those pages' pictures to choose from. None without two pictures yet.
fn which_picture(drill: &Drill, panel: &StoryPanel, shown: &[(StoryPanel, Question)], total: i64, rng: &mut impl rand::Rng) -> Option<Exercise> {
    let drawn: Vec<&StoryPanel> = shown.iter().map(|(p, _)| p).filter(|p| p.image.is_some() && !p.lines.is_empty()).collect();
    let target = *drawn.choose(rng)?;
    let decoy = *drawn.iter().filter(|p| p.id != target.id).collect::<Vec<_>>().choose(rng)?;
    let line = target.lines.choose(rng)?;
    let mut pair = [target, decoy];
    pair.shuffle(rng);
    Some(Exercise {
        prompt: line.pl.clone(),
        hint: "Which picture shows this?".into(),
        answer: target.id.to_string(),
        options: pair.iter().map(|p| p.id.to_string()).collect(),
        option_images: pair.iter().map(|p| p.image).collect(),
        pl: line.pl.clone(),
        meaning: line.en.clone(),
        ..question_card(drill, panel, total, "picture")
    })
}

/// "Which happened first?": a sentence from each of two pages read so far.
fn which_first(drill: &Drill, panel: &StoryPanel, shown: &[(StoryPanel, Question)], total: i64, polish: bool, rng: &mut impl rand::Rng) -> Option<Exercise> {
    let with_lines: Vec<&StoryPanel> = shown.iter().map(|(p, _)| p).filter(|p| !p.lines.is_empty()).collect();
    let mut two: Vec<&StoryPanel> = with_lines.choose_multiple(rng, 2).copied().collect();
    two.sort_by_key(|p| p.number);
    let [first, then] = two.as_slice() else { return None };
    let (a, b) = (first.lines.choose(rng)?, then.lines.choose(rng)?);
    let mut options = vec![a.pl.clone(), b.pl.clone()];
    options.shuffle(rng);
    Some(Exercise {
        prompt: if polish { "Co było najpierw?" } else { "Which happened first?" }.into(),
        hint: "Put the story in order".into(),
        answer: a.pl.clone(),
        options,
        pl: a.pl.clone(),
        meaning: a.en.clone(),
        ..question_card(drill, panel, total, "order")
    })
}

/// The story's words and kept sentences, practised like any drill's: a new
/// word recognised (it was met in the story, and on its last page), a known
/// one practised harder as it comes back; a new sentence built from its
/// words, a known one by the card its familiarity and level give. Without a
/// story (`intro`), new words and sentences are introduced first.
fn practise(drill: &Drill, intro: bool, rng: &mut impl rand::Rng) -> Vec<Exercise> {
    let mut exercises = Vec::new();
    let mut known = Vec::new();
    for (w, is_new) in &drill.words {
        if *is_new {
            if intro {
                exercises.push(session::word_intro(w));
            }
            exercises.push(session::recognise(w, &drill.pool, rng));
        } else {
            known.push(session::practise_word(w, drill, rng));
        }
    }
    known.shuffle(rng);
    exercises.extend(known);
    for (s, is_new) in &drill.sentences {
        if *is_new && intro {
            exercises.push(session::sentence_intro(s));
        }
        // Spec: DRILL-15
        exercises.push(session::practise_sentence(s, *is_new, drill, |k| k != "say-sentence", rng));
    }
    exercises
}

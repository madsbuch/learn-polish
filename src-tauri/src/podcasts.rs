//! Learning podcasts: short spoken episodes that say every sentence first in
//! English and then in Polish, the same pairing that makes the songs work.
//!
//! A writer drafts the script on a subject the learner cares about — from
//! up to five Wikipedia articles, or as a short story with none; the speech model reads each sentence; the clips are
//! joined, with silence between them, into one WAV file. Because the app
//! puts the audio together itself, it knows exactly when each sentence is
//! said, so the lyrics follow along without guessing.
//!
//! An episode is stored as a song of kind "podcast", so the player, playlists,
//! casting, export and listening lessons all work on it unchanged. How its
//! audio is put together (pauses, repeats, speed, voices) is kept with it and
//! can be changed afterwards: every clip is cached, so making the audio again
//! costs nothing.

use diesel::prelude::*;
use futures_util::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::ai;
use crate::db::{Conn, Db};
use crate::error::{Error, Result};
use crate::files;
use crate::schema::{courses, song_tracks, songs as song_rows};
use crate::songs::{self, Song, SongDraft, SongLine, SongOrigin, SongSection, Source, TimedLine};
use crate::speech::{self, Delivery};
use crate::news;
use crate::wiki;
#[allow(unused_imports)]
use crate::ids::*;

/// The formats an episode can take, each a way of writing and reading it
/// (see `format_of`). "pairs": each sentence in English, then in Polish.
/// "immersion": only Polish is spoken, in simpler sentences; the English
/// meaning is shown with each line but not said.
pub const DEFAULT_FORMAT: &str = "pairs";

/// The most of the articles the writer reads, shared between them.
const READING_LIMIT: usize = 16_000;
/// The most articles an episode is written from.
pub const ARTICLE_LIMIT: usize = 5;
/// The most sentence pairs an episode holds.
const LINE_LIMIT: usize = 120;
/// How many clips are made at once.
const PARALLEL_CLIPS: usize = 4;

// Silence, in seconds, around what is said.
const LEAD_IN: f64 = 0.6;
const BETWEEN_REPEATS: f64 = 1.0;
/// Also the quiet at the very end.
const AFTER_PAIR: f64 = 1.2;
/// Between sentences when only Polish is heard.
const AFTER_SENTENCE: f64 = 0.8;
const BETWEEN_PARTS: f64 = 2.0;

/// How an episode's audio is put together. Changing it only joins the cached
/// clips again (a new speed or voice makes the clips it lacks).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct PodcastAudio {
    /// Seconds of silence after the English, before the Polish: time to say
    /// it yourself first.
    pub pause: f64,
    /// How many times the Polish is said.
    pub repeats: u32,
    /// The Polish read slowly.
    pub slow: bool,
    /// "two": the English and the Polish in different voices; "one": both in
    /// the same voice.
    pub voices: String,
    /// The English is said before each Polish sentence. Off: only Polish
    /// is heard (the English meaning still shows with the lyrics).
    pub english: bool,
    /// Who speaks besides the narrator, each with a voice of their own.
    /// Empty: a narrator alone.
    pub cast: Vec<CastMember>,
}

/// Someone who speaks in an episode: a host or a character.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct CastMember {
    pub name: String,
    /// "f" or "m".
    pub gender: String,
    /// One word for how they come across ("curious", "grumpy"); matched to a
    /// voice's character.
    pub character: String,
    /// The Gemini voice they speak in, chosen when the episode is recorded.
    pub voice: String,
}

/// The most people in a cast.
const CAST_LIMIT: usize = 4;

/// Voices for the English when the cast speaks the Polish: calm, clear
/// narrators, the first not in the cast.
const ENGLISH_NARRATORS: &[&str] = &["Charon", "Sulafat", "Iapetus", "Erinome", "Schedar", "Achernar"];

impl Default for PodcastAudio {
    fn default() -> Self {
        Self::for_format(DEFAULT_FORMAT)
    }
}

impl PodcastAudio {
    pub fn for_format(format: &str) -> Self {
        match format {
            "immersion" => {
                Self { pause: 0.0, repeats: 1, slow: false, voices: "two".into(), english: false, cast: Vec::new() }
            }
            _ => Self { pause: 0.8, repeats: 1, slow: false, voices: "two".into(), english: true, cast: Vec::new() },
        }
    }

    /// Keeps the settings within what makes sense, whatever was sent.
    pub fn clean(mut self) -> Self {
        self.pause = if self.pause.is_finite() { self.pause.clamp(0.0, 6.0) } else { 1.0 };
        self.repeats = self.repeats.clamp(1, 3);
        if self.voices != "one" {
            self.voices = "two".into();
        }
        self
    }
}

/// What the composer asks the writer for.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PodcastRequest {
    /// What the episode is about, in the learner's words.
    pub topic: String,
    /// English Wikipedia articles (titles) to write from, up to five; none:
    /// the best match for the topic, unless it is a story.
    pub articles: Vec<String>,
    /// A short story, written from no articles.
    pub story: bool,
    /// Today's news from Polish outlets (about `topic`, if one is given),
    /// with Wikipedia for background.
    pub news: bool,
    /// Who talks: "narrator", "hosts" or "characters"; empty: hosts for
    /// articles, characters for a story.
    pub cast: String,
    /// "pairs" | "immersion".
    pub format: String,
    /// "short" | "medium" | "long".
    pub length: String,
    /// What the script must meet, word for word (the tutor's).
    pub requirements: String,
    /// The episode or song this one varies, if any.
    pub based_on: Option<SongId>,
    /// "composer" or "tutor"; empty: the composer.
    pub made_by: String,
    /// The Polish voice: "f" or "m".
    pub vocal_gender: String,
    /// When refining: the current draft and what to change about it.
    pub previous: Option<SongDraft>,
    pub feedback: String,
}

/// A subject the learner might like an episode about.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct TopicIdea {
    pub topic: String,
    /// English Wikipedia articles to write it from; none for a story.
    pub articles: Vec<String>,
    /// Why it suits the learner, in a few words.
    pub why: String,
}

// ---------------------------------------------------------------------------
// Topics
// ---------------------------------------------------------------------------

/// What the app knows about the learner's interests, for the writer.
fn interests(conn: &mut Conn) -> Result<Vec<String>> {
    if !crate::memories::enabled(conn) {
        return Ok(Vec::new());
    }
    Ok(crate::memories::list(conn)?
        .into_iter()
        .filter(|m| m.category != "struggle" && m.category != "strength")
        .take(30)
        .map(|m| m.content)
        .collect())
}

fn existing_titles(conn: &mut Conn) -> Result<Vec<String>> {
    Ok(song_rows::table
        .filter(song_rows::kind.eq("podcast"))
        .order(song_rows::id.desc())
        .limit(30)
        .select(song_rows::title)
        .load(conn)?)
}

const TOPIC_RULES: &str = r#"You suggest subjects for short learning podcasts in Polish for an adult learner who speaks English. An episode is written from English Wikipedia articles (one to five), or it is a short made-up story written from none.

Suggest 6 subjects. Most should follow what the learner is interested in; one or two may be about Poland (its history, places, nature, people, food or customs) in a way that connects to those interests. Prefer concrete, story-like subjects (a person, a place, an event, an animal, an invention) over abstract ones. Usually one article is enough; give two or three when the subject brings several together (a person and their invention). Make one of the six a short story with no articles. Give the exact titles of real English Wikipedia articles. Avoid subjects that already have an episode.

Reply with ONLY a JSON object:
{"ideas": [{"topic": "short title for the episode", "articles": ["Exact Wikipedia article title"], "why": "a few words on why it suits the learner"}]}"#;

/// Suggests subjects for an episode, from what the learner cares about.
#[tauri::command]
pub async fn podcast_topics(db: State<'_, Db>) -> Result<Vec<TopicIdea>> {
    let (key, model, brief) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let mut brief = String::new();
        let interests = interests(conn)?;
        if interests.is_empty() {
            brief.push_str("Nothing is known about the learner's interests yet: suggest a varied mix.\n");
        } else {
            brief.push_str(&format!("What is known about the learner:\n- {}\n", interests.join("\n- ")));
        }
        let course_titles: Vec<String> =
            courses::table.order(courses::id.desc()).limit(12).select(courses::title_en).load(conn)?;
        if !course_titles.is_empty() {
            brief.push_str(&format!("\nTheir courses: {}\n", course_titles.join(", ")));
        }
        let titles = existing_titles(conn)?;
        if !titles.is_empty() {
            brief.push_str(&format!("\nEpisodes that already exist: {}\n", titles.join(" | ")));
        }
        (ai::api_key(conn)?, songs::songwriter_model(conn)?, brief)
    };
    let messages = [ai::Wire::system(TOPIC_RULES), ai::Wire::user(brief)];
    let reply = ai::complete_json(&key, &model, &messages, 3000).await?;
    let _ = crate::costs::record_tokens(&mut db.lock(), crate::costs::SONGWRITER, &model, reply.cost, "topics", reply.tokens);
    /// The ideas, as the writer was asked for them, or a bare list.
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Ideas {
        Wrapped { ideas: Vec<crate::wire::Lenient<TopicIdea>> },
        List(Vec<crate::wire::Lenient<TopicIdea>>),
    }
    let ideas = match ai::json_in::<Ideas>(&reply.text) {
        Ok(Ideas::Wrapped { ideas } | Ideas::List(ideas)) => ideas,
        Err(_) => {
            crate::diagnostics::unreadable_reply("Podcast ideas", &reply.text);
            return Err(Error::msg("No suggestions came back. Try again."));
        }
    };
    Ok(crate::wire::kept(ideas).filter(|idea| !idea.topic.trim().is_empty()).take(8).collect())
}

// ---------------------------------------------------------------------------
// The script
// ---------------------------------------------------------------------------

/// How long an episode is, as sentence pairs for the writer.
fn pair_count(format: &str, length: &str) -> usize {
    match (format, length) {
        ("immersion", "short") => 30,
        ("immersion", "medium") => 50,
        ("immersion", _) => 80,
        (_, "short") => 20,
        (_, "medium") => 35,
        _ => 55,
    }
}

/// A format as asked for; the early "slow" format is immersion now.
fn format_of(format: &str) -> &'static str {
    match format {
        "immersion" | "slow" => "immersion",
        _ => DEFAULT_FORMAT,
    }
}

fn format_rules(format: &str) -> &'static str {
    match format {
        "immersion" => {
            "Format: Polish only. The learner hears ONLY the Polish; the English is never said, it \
             is shown with each line as its meaning.\n\
             - Simple Polish a beginner can follow by ear: 3–9 words per sentence, one idea each, \
             mostly present tense, the most common words, simple word order.\n\
             - Let the context carry the meaning: say things in an order that makes them easy to \
             guess, and come back to the key words often, in slightly different sentences.\n\
             - Tell it as a story or a calm walk through the subject, concrete and visual.\n\
             - Each line still has \"en\" (its exact meaning, for reading) and \"pl\"."
        }
        _ => {
            "Format: sentence pairs, like the verses of a learning song.\n\
             - 4–12 words per sentence; mostly short, everyday sentences.\n\
             - Tell it as a story: start with something surprising or concrete, then explain.\n\
             - Introduce the key words of the subject early and come back to them."
        }
    }
}

const WRITER_RULES: &str = r#"You write short learning podcasts that teach Polish to an adult who speaks English. Every sentence is written as a pair: the Polish, and the same meaning in English. How the pairs are heard depends on the format below. The learner listens, often more than once, without reading.

Rules:
- The Polish must be natural, grammatical, correct Polish with correct diacritics (ą ć ę ł ń ó ś ź ż) — what a Pole would say, not a word-for-word translation. The English must say the same thing, so the pair teaches.
- Split the episode into 3–6 parts, each with its own small focus. The first part opens the subject; the last part sums it up in a few of the most useful sentences again.
- Work in the learner's words where they fit naturally.
- Names stay as they are; use the Polish form of a name when Poles use one (Kraków, Niemcy, Londyn).
- The title is in English; titlePl is the same title in Polish; about is one English sentence saying what the episode is about.
- "speaker" names who says a line: someone from "cast", or "" for the narrator. Who talks is given below.
- "how" (optional) is how a line is said, in one to three English words — "surprised", "laughing", "whispering", "proudly". Use it where the feeling matters; leave it out for most lines.

Reply with ONLY a JSON object, no prose, in exactly this shape:
{
  "title": "The salt mine under the town",
  "titlePl": "Kopalnia soli pod miastem",
  "about": "How the Wieliczka Salt Mine was dug, and what is inside it today.",
  "cast": [{"name": "Kasia", "gender": "f", "character": "curious"}, {"name": "Marek", "gender": "m", "character": "knowledgeable"}],
  "parts": [
    {"lines": [
      {"speaker": "Kasia", "en": "Have you been down the mine?", "pl": "Byłeś w kopalni?"},
      {"speaker": "Marek", "how": "laughing", "en": "Many times!", "pl": "Wiele razy!"}
    ]}
  ]
}"#;

/// Who talks, as the writer is told it.
fn cast_rules(cast: &str) -> &'static str {
    match cast {
        "narrator" => {
            "Who talks: one narrator tells it all. \"cast\" is empty and every \"speaker\" is \"\"."
        }
        "characters" => {
            "Who talks: a small cast of 2–4 characters with Polish first names (both women and men), \
             and a narrator. Tell it mostly through what the characters say to each other — a radio \
             play — with the narrator (speaker \"\") only for a few lines that set the scene. Give each \
             character a one-word character (\"cheerful\", \"grumpy\", \"shy\", \"wise\") and let it \
             show in what they say."
        }
        _ => {
            "Who talks: two podcast hosts, a woman and a man with Polish first names, talking about the \
             subject together — one curious, asking and reacting; the other knowledgeable, explaining. \
             Short turns, real reactions (\"Really?\", \"Wait — how long?\"), each building on the \
             other. Every line has a speaker; no narrator."
        }
    }
}

/// Who talks, as asked for: hosts discuss articles; a story has characters.
fn cast_of(cast: &str, story: bool) -> &'static str {
    match cast {
        "narrator" => "narrator",
        "hosts" => "hosts",
        "characters" => "characters",
        _ if story => "characters",
        _ => "hosts",
    }
}

const NEWS_PICKER: &str = r#"You pick stories for a short news podcast for an adult learning Polish. You are given the latest headlines from Polish news outlets, numbered, newest first, some with the start of a summary.

Choose 4–6 different stories worth telling: a mix (politics, society, the economy, culture, science, sport, something light) rather than all crime or accidents, and nothing graphic. When several headlines are the same story, group their numbers. Prefer the last day's news; where there is a choice, prefer stories that touch what the learner cares about. If the learner asked for a subject (perhaps in English — the headlines are Polish), choose only stories about it, as few as there are; if none are, reply {"stories": []}.

For each story name 0–2 English Wikipedia articles that give useful background — the people, places or organisations it is about — by their exact titles, only articles that surely exist.

Reply with ONLY a JSON object:
{"stories": [{"headlines": [3, 17], "background": ["Sejm"]}]}"#;

/// The most Wikipedia articles a news episode reads for background.
const BACKGROUND_LIMIT: usize = 5;

/// Today's news for an episode: the stories the picker chose (each one or
/// two headlines, with their pages' summaries) and Wikipedia articles for
/// background.
async fn gather_news(db: &Db, subject: &str) -> Result<(Vec<wiki::Article>, Vec<Vec<news::Headline>>)> {
    let headlines = news::headlines().await?;
    if headlines.is_empty() {
        return Err(Error::msg("The news feeds had no stories just now. Try again in a little while."));
    }
    let (key, model, brief) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let mut brief = String::new();
        if !subject.trim().is_empty() {
            brief.push_str(&format!("The learner asked for news about: {}\n\n", subject.trim()));
        }
        let interests = interests(conn)?;
        if !interests.is_empty() {
            brief.push_str(&format!("What the learner cares about:\n- {}\n\n", interests.join("\n- ")));
        }
        brief.push_str("Headlines:\n");
        for (i, h) in headlines.iter().enumerate() {
            let gist: String = h.summary.chars().take(140).collect();
            if gist.is_empty() {
                brief.push_str(&format!("{i}. {} ({}, {})\n", h.title, h.outlet, h.date));
            } else {
                brief.push_str(&format!("{i}. {} — {gist} ({}, {})\n", h.title, h.outlet, h.date));
            }
        }
        (ai::api_key(conn)?, songs::songwriter_model(conn)?, brief)
    };
    let messages = [ai::Wire::system(NEWS_PICKER), ai::Wire::user(brief)];
    let reply = ai::complete_json(&key, &model, &messages, 3000).await?;
    let _ = crate::costs::record_tokens(&mut db.lock(), crate::costs::SONGWRITER, &model, reply.cost, "news", reply.tokens);
    let (picked, background) = parse_picks(&reply.text, headlines.len());
    if picked.is_empty() {
        return Err(Error::msg(if subject.trim().is_empty() {
            "No stories could be chosen from today's news. Try again.".to_string()
        } else {
            format!("Nothing in the latest news is about “{}”. Try a broader subject, or none.", subject.trim())
        }));
    }

    // A story the feed gives no summary of: the page's own, read side by side.
    let mut stories: Vec<Vec<news::Headline>> =
        picked.iter().map(|ids| ids.iter().map(|&i| headlines[i].clone()).collect()).collect();
    let pages: Vec<(usize, usize, String)> = stories
        .iter()
        .enumerate()
        .flat_map(|(s, story)| {
            story.iter().enumerate().filter(|(_, head)| head.summary.is_empty()).map(move |(h, head)| (s, h, head.url.clone()))
        })
        .collect();
    let summaries = futures_util::future::join_all(pages.iter().map(|(_, _, url)| news::summary(url))).await;
    for ((s, h, _), summary) in pages.iter().zip(summaries) {
        stories[*s][*h].summary = summary.unwrap_or_default();
    }

    let articles = find_articles("", &background).await;
    Ok((articles, stories))
}

/// The picker's choice: stories as headline numbers (in range, each used
/// once, at most two a story, at most six stories), and background articles.
fn parse_picks(reply: &str, count: usize) -> (Vec<Vec<usize>>, Vec<String>) {
    #[derive(Deserialize)]
    struct Picks {
        #[serde(default)]
        stories: Vec<crate::wire::Lenient<Story>>,
    }
    #[derive(Deserialize)]
    struct Story {
        #[serde(default)]
        headlines: Vec<crate::wire::Lenient<u64>>,
        #[serde(default)]
        background: Vec<crate::wire::Lenient<String>>,
    }
    let Ok(picks) = ai::json_in::<Picks>(reply) else {
        crate::diagnostics::unreadable_reply("The news stories picked", reply);
        return (Vec::new(), Vec::new());
    };
    let mut used: Vec<usize> = Vec::new();
    let mut stories = Vec::new();
    let mut background: Vec<String> = Vec::new();
    for story in crate::wire::kept(picks.stories).take(6) {
        let ids: Vec<usize> = crate::wire::kept(story.headlines)
            .map(crate::convert::offset)
            .filter(|&n| n < count)
            .collect();
        let mut fresh = Vec::new();
        for id in ids {
            if !used.contains(&id) && fresh.len() < 2 {
                used.push(id);
                fresh.push(id);
            }
        }
        if fresh.is_empty() {
            continue;
        }
        stories.push(fresh);
        for title in crate::wire::kept(story.background) {
            let title = title.trim().to_string();
            if !title.is_empty() && !background.contains(&title) && background.len() < BACKGROUND_LIMIT {
                background.push(title);
            }
        }
    }
    (stories, background)
}

/// What a news episode is written from: the stories as the news has them,
/// and Wikipedia for background.
fn news_reading(stories: &[Vec<news::Headline>], background: &[wiki::Article], date: &str) -> String {
    let mut out = format!(
        "\nThis is a NEWS episode for {date}. Open with the date (in the Polish too). Tell only what \
         the headlines and summaries below say: add no details, numbers, names or quotes they do not \
         contain, and say where a story comes from (\"according to PAP\", \"według PAP\"). Explain \
         background only from the Wikipedia articles given, and only where it helps. One part per \
         story, with a short opening and a short goodbye.\n"
    );
    for (i, story) in stories.iter().enumerate() {
        out.push_str(&format!("\nStory {}:\n", i + 1));
        for h in story {
            out.push_str(&format!("- “{}” ({}, {})\n", h.title, h.outlet, h.date));
            if !h.summary.is_empty() {
                out.push_str(&format!("  Summary: {}\n", h.summary));
            }
        }
    }
    if !background.is_empty() {
        let share = READING_LIMIT / 2 / background.len();
        out.push_str("\nBackground from Wikipedia:\n");
        for article in background {
            let text: String = article.content.chars().take(share).collect();
            out.push_str(&format!("\n### “{}”\n\n{text}\n", article.title));
        }
    }
    out
}

/// The articles an episode is written from: the ones named (up to five),
/// else the best match for the topic. Empty when Wikipedia has nothing (or
/// is unreachable).
async fn find_articles(topic: &str, titles: &[String]) -> Vec<wiki::Article> {
    let mut found: Vec<wiki::Article> = Vec::new();
    for title in titles.iter().map(|t| t.trim()).filter(|t| !t.is_empty()).take(ARTICLE_LIMIT) {
        if let Ok(article) = wiki::fetch("en", title).await {
            let duplicate = found.iter().any(|a| a.pageid == article.pageid);
            if !article.content.trim().is_empty() && !duplicate {
                found.push(article);
            }
        }
    }
    if !found.is_empty() || topic.trim().is_empty() {
        return found;
    }
    let Ok(hits) = wiki::search("en", topic.trim()).await else { return found };
    for hit in hits.into_iter().take(2) {
        if let Ok(article) = wiki::fetch("en", &hit.title).await {
            if !article.content.trim().is_empty() {
                found.push(article);
                break;
            }
        }
    }
    found
}

/// The articles as the episode records them. The reader shows each article's
/// Polish version beside it, so that is not a source of its own.
fn sources_of(articles: &[wiki::Article]) -> Vec<Source> {
    articles
        .iter()
        .map(|a| Source { title: a.title.clone(), url: a.url.clone(), lang: a.lang.clone() })
        .collect()
}

/// What the writer works from, and how to treat it.
fn reading(articles: &[wiki::Article], story: bool) -> String {
    if story {
        return "\nThis is a short original STORY, not a factual piece: invent characters (with Polish \
                names), a place, a small problem and how it is solved, in everyday situations. \
                Keep it warm and easy to follow by ear.\n"
            .to_string();
    }
    if articles.is_empty() {
        return "\nNo article could be found: keep to well-known, certain facts about the subject.\n".to_string();
    }
    let share = READING_LIMIT / articles.len();
    let mut out = String::from(
        "\nBase the facts on these Wikipedia articles. Do not invent facts; leave out what you are \
         unsure of. Weave them into one episode rather than one part per article.\n",
    );
    for article in articles {
        let text: String = article.content.chars().take(share).collect();
        out.push_str(&format!("\n### “{}”\n\n{text}\n", article.title));
    }
    out
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Script {
    title: String,
    title_pl: String,
    about: String,
    cast: Vec<CastMember>,
    parts: Vec<ScriptPart>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ScriptPart {
    lines: Vec<SongLine>,
}

fn parse_script(reply: &str) -> Result<Script> {
    let script = match ai::json_text(reply) {
        None => Err(Error::msg("The writer did not return a script. Try again.")),
        Some(json) => serde_json::from_str::<Script>(json)
            .map_err(|e| Error::msg(format!("The writer's script was not readable ({e}). Try again."))),
    };
    if script.is_err() {
        crate::diagnostics::unreadable_reply("A podcast script", reply);
    }
    script
}

/// The script as an editable draft.
fn script_to_draft(script: Script, format: &str, vocal_gender: &str) -> SongDraft {
    SongDraft {
        title: script.title,
        chorus_pl: script.title_pl,
        chorus_en: script.about,
        style: format.to_string(),
        vocal_gender: vocal_gender.to_string(),
        sections: script
            .parts
            .into_iter()
            .map(|part| SongSection { kind: "verse".into(), lines: part.lines })
            .collect(),
        kind: "podcast".into(),
        podcast: Some(PodcastAudio { cast: script.cast, ..PodcastAudio::for_format(format) }),
        ..Default::default()
    }
}

/// The cast as it is kept: named, at most four, no one twice, and anyone
/// who speaks but was not listed added (with a guess at nothing).
fn tidy_cast(cast: Vec<CastMember>, sections: &[SongSection]) -> Vec<CastMember> {
    let mut tidy: Vec<CastMember> = Vec::new();
    let mut add = |member: CastMember| {
        let name = member.name.trim().to_string();
        if name.is_empty() || tidy.iter().any(|m| m.name.eq_ignore_ascii_case(&name)) || tidy.len() >= CAST_LIMIT {
            return;
        }
        let gender = if member.gender.trim().to_lowercase().starts_with('m') { "m" } else { "f" };
        tidy.push(CastMember {
            name,
            gender: gender.into(),
            character: member.character.trim().to_lowercase(),
            voice: member.voice.trim().to_string(),
        });
    };
    for member in cast {
        add(member);
    }
    for line in sections.iter().flat_map(|s| &s.lines) {
        if !line.speaker.is_empty() {
            add(CastMember { name: line.speaker.clone(), ..Default::default() });
        }
    }
    tidy
}

/// Gives each of the cast a voice of their own: a gender match, the learner's
/// chosen voice for the first of each gender, else one whose character fits,
/// else the next free one — never one already taken (the narrator's).
pub fn assign_voices(cast: &mut [CastMember], taken: &[String], chosen_female: &str, chosen_male: &str) {
    let mut used: Vec<String> = taken.to_vec();
    used.extend(cast.iter().filter(|m| !m.voice.is_empty()).map(|m| m.voice.clone()));
    for member in cast.iter_mut().filter(|m| m.voice.is_empty()) {
        let gender = if member.gender == "m" { "m" } else { "f" };
        let free = |name: &str| !used.iter().any(|u| u == name);
        let candidates: Vec<&(&str, &str, &str)> =
            ai::GEMINI_VOICES.iter().filter(|(name, g, _)| *g == gender && free(name)).collect();
        let chosen = if gender == "m" { chosen_male } else { chosen_female };
        let pick = candidates
            .iter()
            .find(|(name, _, _)| *name == chosen)
            .or_else(|| candidates.iter().find(|(_, _, character)| *character == member.character))
            .or_else(|| candidates.first())
            .map(|(name, _, _)| name.to_string())
            // More people of one gender than voices: share.
            .unwrap_or_else(|| chosen.to_string());
        used.push(pick.clone());
        member.voice = pick;
    }
}

/// The draft as it is stored: a podcast, trimmed, nothing empty, within limits.
pub fn normalize_episode(mut draft: SongDraft) -> Result<SongDraft> {
    draft.kind = "podcast".into();
    draft.title = draft.title.trim().chars().take(80).collect();
    draft.chorus_pl = draft.chorus_pl.trim().to_string();
    draft.chorus_en = draft.chorus_en.trim().to_string();
    draft.style = format_of(&draft.style).into();
    draft.vocal_gender = if draft.vocal_gender == "m" { "m".into() } else { "f".into() };
    for section in &mut draft.sections {
        section.kind = "verse".into();
        for line in &mut section.lines {
            line.en = line.en.trim().to_string();
            line.pl = line.pl.trim().to_string();
            line.speaker = line.speaker.trim().to_string();
            line.how = line.how.trim().chars().take(40).collect();
            // A narrator by any other name is the narrator.
            if ["narrator", "lektor", "narrator:"].contains(&line.speaker.to_lowercase().as_str()) {
                line.speaker.clear();
            }
        }
        section.lines.retain(|l| !l.en.is_empty() || !l.pl.is_empty());
    }
    draft.sections.retain(|s| !s.lines.is_empty());
    let lines: usize = draft.sections.iter().map(|s| s.lines.len()).sum();
    if lines == 0 {
        return Err(Error::msg("The episode has no sentences yet."));
    }
    if lines > LINE_LIMIT {
        return Err(Error::msg(format!("The episode has {lines} sentences; keep it to {LINE_LIMIT}.")));
    }
    if draft.title.is_empty() {
        draft.title = draft.chorus_pl.chars().take(80).collect();
    }
    if draft.title.is_empty() {
        return Err(Error::msg("Give the episode a title."));
    }
    let mut audio = draft.podcast.take().unwrap_or_else(|| PodcastAudio::for_format(&draft.style));
    audio.cast = tidy_cast(std::mem::take(&mut audio.cast), &draft.sections);
    draft.podcast = Some(audio.clean());
    Ok(draft)
}

/// Asks the writer for a script, or a revision of one.
#[tauri::command]
pub async fn podcast_draft(db: State<'_, Db>, request: PodcastRequest) -> Result<SongDraft> {
    write_episode(&db, request).await
}

/// The writer at work: used by the composer and by the tutor.
pub async fn write_episode(db: &Db, request: PodcastRequest) -> Result<SongDraft> {
    if let Some(previous) = &request.previous {
        return revise(db, previous, &request.feedback, &request.made_by).await;
    }
    let format = format_of(&request.format);
    let length = if songs::SONG_LENGTHS.contains(&request.length.as_str()) { request.length.as_str() } else { "medium" };
    if !request.news && request.topic.trim().is_empty() && request.articles.iter().all(|a| a.trim().is_empty()) {
        return Err(Error::msg("Say what the episode should be about."));
    }

    let (articles, stories) = if request.news {
        gather_news(db, &request.topic).await?
    } else if request.story {
        (Vec::new(), Vec::new())
    } else {
        (find_articles(&request.topic, &request.articles).await, Vec::new())
    };

    let (key, model, brief) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let context = songs::gather_context(conn)?;
        let mut brief = String::new();
        let subject = match (request.news, request.topic.trim()) {
            (true, "") => format!("today's news from {}", news::PLACE),
            (true, topic) => format!("today's news from {} about {topic}", news::PLACE),
            (false, "") => request.articles.join(", "),
            (false, topic) => topic.to_string(),
        };
        brief.push_str(&format!("The episode is about: {subject}\n\n"));
        brief.push_str(format_rules(format));
        brief.push_str("\n");
        brief.push_str(cast_rules(cast_of(&request.cast, request.story)));
        brief.push_str(&format!(
            "\n\nLength: about {} sentence pairs in all.\n",
            pair_count(format, length)
        ));
        let interests = interests(conn)?;
        if !interests.is_empty() {
            brief.push_str(&format!(
                "\nWhat is known about the learner (choose angles and examples that suit them):\n- {}\n",
                interests.join("\n- ")
            ));
        }
        let words: Vec<String> = context.words.iter().take(25).cloned().collect();
        if !words.is_empty() {
            brief.push_str(&format!("\nWords the learner knows or is learning:\n{}\n", words.join("\n")));
        }
        let read = if request.news {
            let (y, m, d) = crate::commands::civil_date(crate::db::today(conn));
            news_reading(&stories, &articles, &format!("{y}-{m:02}-{d:02}"))
        } else {
            reading(&articles, request.story)
        };
        brief.push_str(&read);
        brief.push_str(&songs::requirements_text(&request.requirements));
        // What came from the app, kept with the episode: with the request it
        // rebuilds this prompt.
        let given = songs::WriterInput { words, memories: interests, reading: read, ..Default::default() };
        (ai::api_key(conn)?, songs::songwriter_model(conn)?, (brief, given))
    };
    let (brief, given) = brief;

    let messages = [ai::Wire::system(WRITER_RULES), ai::Wire::user(brief)];
    let reply = ai::complete_json(&key, &model, &messages, 12_000).await?;
    let _ = crate::costs::record_tokens(&mut db.lock(), crate::costs::SONGWRITER, &model, reply.cost, "podcast", reply.tokens);
    if reply.truncated {
        return Err(Error::msg("The script was cut off. Try a shorter episode."));
    }
    let mut draft = script_to_draft(parse_script(&reply.text)?, format, &request.vocal_gender);
    draft.course_id = None;
    draft.origin = SongOrigin {
        brief: request.topic.trim().to_string(),
        length: length.to_string(),
        songwriter_model: model,
        sources: sources_of(&articles),
        // The first story's picture gives the cover, else the first article's.
        source_image: stories
            .iter()
            .flatten()
            .map(|h| h.image.clone())
            .find(|i| !i.is_empty())
            .or_else(|| articles.iter().find_map(|a| a.thumbnail.clone()))
            .unwrap_or_default(),
        news: request.news,
        news_items: stories.into_iter().flatten().collect(),
        requested_articles: request.articles.clone(),
        story: request.story,
        requested_cast: cast_of(&request.cast, request.story).to_string(),
        made_by: songs::made_by(&request.made_by).into(),
        requirements: request.requirements.trim().to_string(),
        based_on: request.based_on,
        given,
        instructions_version: songs::instructions_version(WRITER_RULES),
        ..Default::default()
    };
    normalize_episode(draft)
}

/// Makes a whole episode from a request, in one go: the episode appears in the
/// library at once, and is written and then recorded in the background (the
/// app keeps working with the screen off or another app in front). There is
/// no script to approve: a learner who does not like it deletes it.
#[tauri::command]
pub async fn podcast_make(app: AppHandle, db: State<'_, Db>, request: PodcastRequest) -> Result<Song> {
    let id = start_from_request(&app, &db, request)?;
    songs::load_song(&mut db.lock(), id)
}

pub fn start_from_request(app: &AppHandle, db: &Db, request: PodcastRequest) -> Result<SongId> {
    let articles: Vec<String> = request
        .articles
        .iter()
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
        .take(ARTICLE_LIMIT)
        .collect();
    let topic = request.topic.trim().to_string();
    if !request.news && topic.is_empty() && articles.is_empty() {
        return Err(Error::msg("Say what the episode should be about."));
    }
    let format = format_of(&request.format).to_string();
    let length = if songs::SONG_LENGTHS.contains(&request.length.as_str()) { request.length.clone() } else { "medium".into() };
    // A placeholder until the script is written: the subject as its title.
    let placeholder = SongDraft {
        title: match (request.news, topic.is_empty()) {
            (true, true) => format!("News from {}", news::PLACE),
            (true, false) => format!("News: {topic}"),
            (false, true) => articles.join(", "),
            (false, false) => topic.clone(),
        }
        .chars()
        .take(80)
        .collect(),
        style: format.clone(),
        vocal_gender: if request.vocal_gender == "m" { "m".into() } else { "f".into() },
        kind: "podcast".into(),
        podcast: Some(PodcastAudio::for_format(&format)),
        origin: SongOrigin {
            brief: topic,
            length,
            requested_articles: if request.story || request.news { Vec::new() } else { articles },
            news: request.news,
            story: request.story,
            requested_cast: cast_of(&request.cast, request.story).to_string(),
            made_by: songs::made_by(&request.made_by).into(),
            requirements: request.requirements.trim().to_string(),
            based_on: request.based_on,
            ..Default::default()
        },
        ..Default::default()
    };
    let id = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        ai::api_key(conn)?;
        songs::songwriter_model(conn)?;
        songs::insert_song(conn, &placeholder, ai::SPEECH_MODEL)?
    };
    spawn_render(app.clone(), id);
    songs::notify(app, id);
    Ok(id)
}

/// Writes the script of an episode that has none yet, from what was asked
/// for (kept with it), and stores it in place of the placeholder.
async fn write_script(db: &Db, song: &Song) -> Result<()> {
    let request = PodcastRequest {
        topic: song.origin.brief.clone(),
        articles: song.origin.requested_articles.clone(),
        story: song.origin.story,
        news: song.origin.news,
        cast: song.origin.requested_cast.clone(),
        format: song.style.clone(),
        length: song.origin.length.clone(),
        requirements: song.origin.requirements.clone(),
        based_on: song.origin.based_on,
        made_by: song.origin.made_by.clone(),
        vocal_gender: song.vocal_gender.clone(),
        ..Default::default()
    };
    let draft = write_episode(db, request).await?;
    // The sound stays as it was set; the cast comes with the script.
    let audio = PodcastAudio {
        cast: draft.podcast.clone().unwrap_or_default().cast,
        ..song.podcast.clone().unwrap_or_default()
    };
    let mut guard = db.lock();
    let conn = &mut *guard;
    diesel::update(song_rows::table.find(song.id))
        .set((
            song_rows::title.eq(&draft.title),
            song_rows::chorus_pl.eq(&draft.chorus_pl),
            song_rows::chorus_en.eq(&draft.chorus_en),
            song_rows::sections.eq(serde_json::to_string(&draft.sections)?),
            song_rows::lyrics.eq(songs::render_lyrics(&draft)),
            song_rows::origin.eq(serde_json::to_string(&draft.origin)?),
            song_rows::podcast.eq(serde_json::to_string(&audio)?),
        ))
        .execute(conn)?;
    Ok(())
}

async fn revise(db: &Db, previous: &SongDraft, feedback: &str, by: &str) -> Result<SongDraft> {
    let feedback = match feedback.trim() {
        "" => "Make it more interesting.",
        text => text,
    };
    let (key, model) = {
        let mut guard = db.lock();
        (ai::api_key(&mut guard)?, songs::songwriter_model(&mut guard)?)
    };
    let current = serde_json::json!({
        "title": previous.title,
        "titlePl": previous.chorus_pl,
        "about": previous.chorus_en,
        "parts": previous.sections.iter().map(|s| serde_json::json!({ "lines": s.lines })).collect::<Vec<_>>(),
    });
    let brief = format!(
        "{}\n\nHere is the current script:\n{}\n\nRevise it as follows, keeping everything else:\n{feedback}\n{}\nReturn the whole revised script as JSON.",
        format_rules(&previous.style),
        serde_json::to_string_pretty(&current).unwrap_or_default(),
        songs::requirements_text(&previous.origin.requirements),
    );
    let messages = [ai::Wire::system(WRITER_RULES), ai::Wire::user(brief)];
    let reply = ai::complete_json(&key, &model, &messages, 12_000).await?;
    let _ = crate::costs::record_tokens(&mut db.lock(), crate::costs::SONGWRITER, &model, reply.cost, "podcast", reply.tokens);
    let revised = script_to_draft(parse_script(&reply.text)?, &previous.style, &previous.vocal_gender);
    let mut origin = previous.origin.clone();
    origin.revisions.push(feedback.to_string());
    origin.steps.push(songs::OriginStep {
        kind: "revise".into(),
        by: songs::made_by(by).into(),
        text: feedback.to_string(),
        model: model.clone(),
        before: songs::bare(previous),
    });
    origin.songwriter_model = model;
    normalize_episode(SongDraft {
        course_id: previous.course_id,
        origin,
        podcast: previous.podcast.clone(),
        ..revised
    })
}

// ---------------------------------------------------------------------------
// Making the audio
// ---------------------------------------------------------------------------

/// Saves the episode and starts reading it. Returns at once; progress comes
/// as `songs://changed` events, like a song's.
#[tauri::command]
pub async fn podcast_create(app: AppHandle, db: State<'_, Db>, draft: SongDraft) -> Result<Song> {
    let id = start_episode(&app, &db, draft)?;
    songs::load_song(&mut db.lock(), id)
}

/// Saves a finished script and starts reading it in the background.
pub fn start_episode(app: &AppHandle, db: &Db, draft: SongDraft) -> Result<SongId> {
    let draft = normalize_episode(draft)?;
    let id = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        ai::api_key(conn)?;
        songs::insert_song(conn, &draft, ai::SPEECH_MODEL)?
    };
    spawn_render(app.clone(), id);
    songs::notify(app, id);
    Ok(id)
}

/// Puts the episode's audio together again, with new settings if given.
#[tauri::command]
pub async fn podcast_rerender(
    app: AppHandle,
    db: State<'_, Db>,
    song_id: SongId,
    audio: Option<PodcastAudio>,
) -> Result<Song> {
    rerender(&app, &db, song_id, audio)
}

pub(crate) fn rerender(app: &AppHandle, db: &Db, song_id: SongId, audio: Option<PodcastAudio>) -> Result<Song> {
    {
        let mut guard = db.lock();
        let conn = &mut *guard;
        ai::api_key(conn)?;
        if let Some(audio) = audio {
            diesel::update(song_rows::table.find(song_id))
                .set(song_rows::podcast.eq(serde_json::to_string(&audio.clean())?))
                .execute(conn)?;
        }
        songs::set_status(conn, song_id, "generating", None)?;
    }
    spawn_render(app.clone(), song_id);
    songs::notify(app, song_id);
    songs::load_song(&mut db.lock(), song_id)
}

pub fn spawn_render(app: AppHandle, song_id: SongId) {
    // Keeps the app working with the screen off or another app in front.
    let busy = crate::work::begin(&app, "Making a podcast…");
    tauri::async_runtime::spawn(async move {
        let _busy = busy;
        if let Err(error) = render(&app, song_id).await {
            let db = app.state::<Db>();
            let _ = songs::set_status(&mut db.lock(), song_id, "failed", Some(&error.to_string()));
            songs::notify(&app, song_id);
        }
    });
}

/// Keeps the articles an episode was written from as its sources, for the
/// in-app reader: each with its layout, its Polish version and its picture.
/// Done once, while the episode is recorded; an article that cannot be
/// fetched now is simply left out (its link stays in the episode's origin).
async fn keep_sources(db: &Db, song_id: SongId, sources: &[Source]) {
    let kept = wiki::sources_of_song(&mut db.lock(), song_id).unwrap_or_default();
    // Episodes from before kept the Polish article as a source of its own.
    let wanted: Vec<&Source> = sources.iter().filter(|s| s.lang != "pl" || sources.len() == 1).collect();
    for source in wanted {
        if kept.iter().any(|k| k.lang == source.lang && k.title == source.title) {
            continue;
        }
        let _ = wiki::attach_to(db, wiki::Owner::Song(song_id), &source.lang, &source.title).await;
    }
}

/// One thing to say, in the order the episode says it.
struct Clip {
    text: String,
    voice: String,
    delivery: Delivery,
    /// How it is said ("surprised"), or empty.
    how: String,
}

/// A clip, made or taken from the cache. Owns what it needs, so many can be
/// made at once from a detached task.
async fn make_clip(app: AppHandle, clip: Clip) -> Result<Vec<u8>> {
    let db = app.state::<Db>();
    speech::pcm_clip(&db, &clip.text, &clip.voice, clip.delivery, &clip.how).await
}

async fn render(app: &AppHandle, song_id: SongId) -> Result<()> {
    let db = app.state::<Db>();
    // Made in one go: first the script, if it is not written yet.
    let unwritten = songs::load_song(&mut db.lock(), song_id)?;
    if unwritten.sections.is_empty() {
        write_script(&db, &unwritten).await?;
        songs::notify(app, song_id);
    }
    let (song, audio, english_voice, polish_voice, old) = {
        let mut guard = db.lock();
        let conn = &mut *guard;
        let song = songs::load_song(conn, song_id)?;
        let mut audio = song.podcast.clone().unwrap_or_default().clean();
        let polish_gender = if song.vocal_gender == "m" { "m" } else { "f" };
        let english_gender = match (audio.voices.as_str(), polish_gender) {
            ("one", gender) => gender,
            (_, "m") => "f",
            _ => "m",
        };
        // The narrator's voice; the cast each get one of their own, kept so
        // that making the audio again sounds the same.
        let narrator = ai::voice_for(conn, polish_gender);
        if audio.cast.iter().any(|m| m.voice.is_empty()) {
            let narrated = song.sections.iter().flat_map(|s| &s.lines).any(|l| l.speaker.is_empty());
            let taken = if narrated { vec![narrator.clone()] } else { Vec::new() };
            let (female, male) = (ai::voice_for(conn, "f"), ai::voice_for(conn, "m"));
            assign_voices(&mut audio.cast, &taken, &female, &male);
            diesel::update(song_rows::table.find(song_id))
                .set(song_rows::podcast.eq(serde_json::to_string(&audio)?))
                .execute(conn)?;
        }
        let old: Option<(Option<FileId>, Option<FileId>)> = song_tracks::table
            .filter(song_tracks::song_id.eq(song_id))
            .filter(song_tracks::remote_id.eq("episode"))
            .select((song_tracks::audio_file_id, song_tracks::image_file_id))
            .first(conn)
            .optional()?;
        // With a cast, the English is a calm narrator of its own, apart
        // from everyone who speaks Polish.
        let english_voice = if audio.cast.is_empty() {
            ai::voice_for(conn, english_gender)
        } else {
            ENGLISH_NARRATORS
                .iter()
                .find(|v| **v != narrator && !audio.cast.iter().any(|m| m.voice == **v))
                .map(|v| v.to_string())
                .unwrap_or_else(|| ai::voice_for(conn, english_gender))
        };
        (song, audio, english_voice, narrator, old)
    };

    keep_sources(&db, song_id, &song.origin.sources).await;
    // Its page lists them now, in the reader.
    songs::notify(app, song_id);

    let polish = if audio.slow { Delivery::PolishSlow } else { Delivery::Polish };
    let mut jobs = Vec::new();
    for section in &song.sections {
        for line in &section.lines {
            let speaker_voice = audio
                .cast
                .iter()
                .find(|m| m.name == line.speaker)
                .map(|m| m.voice.clone())
                .unwrap_or_else(|| polish_voice.clone());
            if !line.en.is_empty() && (audio.english || line.pl.is_empty()) {
                // One voice: everyone says their own English too.
                let voice = if audio.voices == "one" { speaker_voice.clone() } else { english_voice.clone() };
                jobs.push(Clip { text: line.en.clone(), voice, delivery: Delivery::English, how: String::new() });
            }
            if !line.pl.is_empty() {
                jobs.push(Clip { text: line.pl.clone(), voice: speaker_voice, delivery: polish, how: line.how.clone() });
            }
        }
    }
    let clips: Vec<Vec<u8>> = stream::iter(jobs)
        .map(|job| make_clip(app.clone(), job))
        .buffered(PARALLEL_CLIPS)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<_>>()?;

    let Episode { wav, duration, lines } = assemble(&song.sections, &clips, &audio)?;
    drop(clips);
    // Stored as MP3, like all speech (audio.rs). An episode is minutes of
    // audio: encoded off the async runtime.
    let (audio_mime, audio_bytes) = tauri::async_runtime::spawn_blocking(move || crate::audio::compact(crate::audio::WAV.to_string(), wav))
        .await
        .map_err(|e| Error::msg(format!("Could not store the episode's audio: {e}")))?;

    // The article's picture is the cover; one already downloaded is kept.
    let old_image = old.and_then(|(_, image)| image);
    let image = match (old_image, song.origin.source_image.as_str()) {
        (None, url) if !url.is_empty() => songs::download(url).await.ok().map(|(mime, bytes)| (url.to_string(), mime, bytes)),
        _ => None,
    };

    let mut guard = db.lock();
    let conn = &mut *guard;
    let dir = files::media_dir(app)?;
    let audio_name = format!("podcast-{song_id}.{}", files::extension_for(&audio_mime));
    let audio_id = files::store(conn, files::SONG_AUDIO, &audio_name, &audio_mime, &audio_bytes)?;
    files::materialize(conn, &dir, audio_id)?;
    let image_id = match image {
        Some((url, mime, bytes)) => {
            let ext = songs::extension(&url, "jpg");
            let mime = mime.unwrap_or_else(|| songs::mime_for(&ext, "image/jpeg"));
            let id = files::store(conn, files::SONG_IMAGE, &format!("podcast-{song_id}.{ext}"), &mime, &bytes)?;
            files::materialize(conn, &dir, id)?;
            Some(id)
        }
        None => old_image,
    };
    let lines = serde_json::to_string(&lines)?;
    diesel::insert_into(song_tracks::table)
        .values((
            song_tracks::id.eq(TrackId::new()),
            song_tracks::song_id.eq(song_id),
            song_tracks::ord.eq(0),
            song_tracks::remote_id.eq("episode"),
            song_tracks::duration.eq(duration),
            song_tracks::audio_file_id.eq(audio_id),
            song_tracks::image_file_id.eq(image_id),
            song_tracks::timed_lines.eq(&lines),
        ))
        .on_conflict((song_tracks::song_id, song_tracks::remote_id))
        .do_update()
        .set((
            song_tracks::duration.eq(duration),
            song_tracks::audio_file_id.eq(audio_id),
            song_tracks::image_file_id.eq(image_id),
            song_tracks::timed_lines.eq(&lines),
        ))
        .execute(conn)?;
    if let Some((Some(old_audio), _)) = old {
        if old_audio != audio_id {
            files::delete(conn, &[old_audio])?;
        }
    }
    songs::set_status(conn, song_id, "ready", None)?;
    drop(guard);
    songs::notify(app, song_id);
    Ok(())
}

/// An episode's audio, put together.
#[derive(Debug)]
pub struct Episode {
    pub wav: Vec<u8>,
    pub duration: f64,
    pub lines: Vec<TimedLine>,
}

/// Audio being put together: 16-bit samples in one format, and the time.
struct Tape {
    format: Option<(u32, u16)>,
    pcm: Vec<u8>,
}

impl Tape {
    /// Where the tape is, in seconds.
    fn now(&self) -> f64 {
        match self.format {
            Some((rate, channels)) => crate::convert::real_len(self.pcm.len()) / (2.0 * f64::from(channels)) / f64::from(rate),
            None => 0.0,
        }
    }

    fn silence(&mut self, seconds: f64) {
        if let Some((rate, channels)) = self.format {
            let frames = crate::convert::round_len(seconds.max(0.0) * f64::from(rate));
            self.pcm.resize(self.pcm.len() + frames * usize::from(channels) * 2, 0);
        }
    }

    /// Plays a clip onto the tape and returns when it starts and ends. The
    /// first clip sets the format, after a moment of silence.
    fn record(&mut self, wav: &[u8]) -> Result<(f64, f64)> {
        let (rate, channels, data) = crate::audio::pcm_of(wav)?;
        match self.format {
            None => {
                self.format = Some((rate, channels));
                self.silence(LEAD_IN);
            }
            Some(known) if known != (rate, channels) => {
                return Err(Error::msg("The speech clips came in different audio formats."));
            }
            Some(_) => {}
        }
        let start = self.now();
        self.pcm.extend_from_slice(data);
        Ok((start, self.now()))
    }
}

/// Joins the clips (one per English and per Polish sentence, in the order
/// `render` made them) into one episode, with the pauses `audio` asks for,
/// and notes when each sentence is said.
pub fn assemble(sections: &[SongSection], clips: &[Vec<u8>], audio: &PodcastAudio) -> Result<Episode> {
    let mut tape = Tape { format: None, pcm: Vec::new() };
    let mut lines = Vec::new();
    let mut clips = clips.iter();
    let mut next = || clips.next().ok_or_else(|| Error::msg("A sentence has no audio."));
    let parts = sections.len();

    for (part, section) in sections.iter().enumerate() {
        if part > 0 {
            tape.silence(BETWEEN_PARTS - if audio.english { AFTER_PAIR } else { AFTER_SENTENCE });
        }
        for (index, line) in section.lines.iter().enumerate() {
            let mut label = (index == 0 && parts > 1).then(|| format!("Part {}", part + 1));
            // Without the English, only a line that has no Polish says it.
            let say_english = !line.en.is_empty() && (audio.english || line.pl.is_empty());
            if say_english {
                let (start, end) = tape.record(next()?)?;
                lines.push(TimedLine {
                    start,
                    end,
                    text: line.en.clone(),
                    lang: "en".into(),
                    gloss: None,
                    section: label.take(),
                    // Said by the speaker only when there is no Polish.
                    speaker: (line.pl.is_empty() && !line.speaker.is_empty()).then(|| line.speaker.clone()),
                });
                if !line.pl.is_empty() {
                    tape.silence(audio.pause);
                }
            }
            if !line.pl.is_empty() {
                let clip = next()?;
                let (start, mut end) = tape.record(clip)?;
                // Said again: one line in the lyrics, held until the last time.
                for _ in 1..audio.repeats.max(1) {
                    tape.silence(BETWEEN_REPEATS);
                    end = tape.record(clip)?.1;
                }
                lines.push(TimedLine {
                    start,
                    end,
                    text: line.pl.clone(),
                    lang: "pl".into(),
                    gloss: (!line.en.is_empty()).then(|| line.en.clone()),
                    section: label.take(),
                    speaker: (!line.speaker.is_empty()).then(|| line.speaker.clone()),
                });
            }
            tape.silence(if audio.english { AFTER_PAIR } else { AFTER_SENTENCE });
        }
    }
    let (rate, channels) = tape.format.ok_or_else(|| Error::msg("The episode has nothing to say."))?;
    let duration = tape.now();
    Ok(Episode { wav: ai::wav_from_pcm(&tape.pcm, rate, channels), duration, lines })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clip of `frames` frames of a constant sample, 1 kHz mono.
    fn clip(frames: usize) -> Vec<u8> {
        ai::wav_from_pcm(&vec![1u8; frames * 2], 1000, 1)
    }

    fn line(en: &str, pl: &str) -> SongLine {
        SongLine { en: en.into(), pl: pl.into(), ..Default::default() }
    }

    #[test]
    fn wav_clips_are_read_back() {
        let wav = clip(10);
        let (rate, channels, data) = crate::audio::pcm_of(&wav).unwrap();
        assert_eq!((rate, channels, data.len()), (1000, 1, 20));
        assert!(crate::audio::pcm_of(b"not a wav file").is_err());
    }

    #[test]
    fn english_then_polish_with_pauses_and_exact_times() {
        let sections = vec![
            SongSection { kind: "verse".into(), lines: vec![line("Hello", "Cześć"), line("", "Dziękuję")] },
            SongSection { kind: "verse".into(), lines: vec![line("Bye", "Do widzenia")] },
        ];
        // Hello, Cześć, Dziękuję, Bye, Do widzenia: 500 ms each.
        let clips: Vec<Vec<u8>> = (0..5).map(|_| clip(500)).collect();
        let audio = PodcastAudio { pause: 1.0, repeats: 2, slow: false, voices: "two".into(), english: true, cast: Vec::new() };
        let episode = assemble(&sections, &clips, &audio).unwrap();
        let times: Vec<(&str, &str, f64, f64, Option<&str>, Option<&str>)> = episode
            .lines
            .iter()
            .map(|l| (l.text.as_str(), l.lang.as_str(), l.start, l.end, l.gloss.as_deref(), l.section.as_deref()))
            .collect();
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        let expected = [
            ("Hello", "en", 0.6, 1.1, None, Some("Part 1")),
            // pause 1.0, then said twice with 1.0 between.
            ("Cześć", "pl", 2.1, 4.1, Some("Hello"), None),
            ("Dziękuję", "pl", 5.3, 7.3, None, None),
            // 1.2 after the pair, 0.8 more between parts.
            ("Bye", "en", 9.3, 9.8, None, Some("Part 2")),
            ("Do widzenia", "pl", 10.8, 12.8, Some("Bye"), None),
        ];
        assert_eq!(times.len(), expected.len());
        for (got, want) in times.iter().zip(expected.iter()) {
            assert_eq!((got.0, got.1, got.4, got.5), (want.0, want.1, want.4, want.5));
            assert!(close(got.2, want.2) && close(got.3, want.3), "{got:?} vs {want:?}");
        }
        assert!(close(episode.duration, 14.0), "a pause after the last line: {}", episode.duration);
        assert_eq!(crate::audio::pcm_of(&episode.wav).unwrap().2.len(), 14_000 * 2);
    }

    #[test]
    fn immersion_says_only_the_polish_but_shows_the_meaning() {
        let sections = vec![SongSection { kind: "verse".into(), lines: vec![line("Hello", "Cześć"), line("Thanks", "Dziękuję")] }];
        // Only the two Polish clips are made.
        let clips: Vec<Vec<u8>> = (0..2).map(|_| clip(500)).collect();
        let episode = assemble(&sections, &clips, &PodcastAudio::for_format("immersion")).unwrap();
        let lines: Vec<(&str, &str, Option<&str>)> =
            episode.lines.iter().map(|l| (l.text.as_str(), l.lang.as_str(), l.gloss.as_deref())).collect();
        assert_eq!(lines, [("Cześć", "pl", Some("Hello")), ("Dziękuję", "pl", Some("Thanks"))]);
        assert!((episode.lines[1].start - (0.6 + 0.5 + AFTER_SENTENCE)).abs() < 1e-9);
    }

    #[test]
    fn everyone_in_the_cast_gets_a_voice_of_their_own() {
        let member = |name: &str, gender: &str, character: &str| CastMember {
            name: name.into(),
            gender: gender.into(),
            character: character.into(),
            voice: String::new(),
        };
        let mut cast = vec![
            member("Kasia", "f", "curious"),
            member("Marek", "m", "gravelly"),
            member("Sofie", "f", "warm"),
            member("Lars", "m", "upbeat"),
        ];
        // The narrator reads in Kore; the learner chose Kore and Puck.
        assign_voices(&mut cast, &["Kore".into()], "Kore", "Puck");
        let voices: Vec<&str> = cast.iter().map(|m| m.voice.as_str()).collect();
        assert_eq!(voices[1], "Puck", "the learner's chosen male voice, free");
        assert_eq!(voices[2], "Sulafat", "a female voice whose character fits");
        assert_eq!(voices[3], "Achird", "Puck is taken and no other voice is upbeat: the first free one");
        assert!(!voices.contains(&"Kore"), "the narrator's voice is not reused");
        let mut unique = voices.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), 4, "{voices:?}");
        for m in &cast {
            assert!(ai::GEMINI_VOICES.iter().any(|(n, g, _)| *n == m.voice && *g == m.gender), "{m:?}");
        }
        // Voices already chosen stay.
        let before = cast.clone();
        assign_voices(&mut cast, &["Kore".into()], "Kore", "Puck");
        assert_eq!(cast, before);
    }

    #[test]
    fn scripts_with_a_cast_keep_who_says_what_and_how() {
        let reply = r#"{"title": "The bridge", "titlePl": "Kopalnia", "about": "A mine.",
            "cast": [{"name": "Kasia", "gender": "female", "character": "Curious"}, {"name": "Kasia", "gender": "f"}],
            "parts": [{"lines": [
                {"speaker": "Narrator", "en": "A mine.", "pl": "Kopalnia."},
                {"speaker": "Kasia", "how": "surprised", "en": "So deep!", "pl": "Tak głęboko!"},
                {"speaker": "Marek", "en": "Yes.", "pl": "Tak."}
            ]}]}"#;
        let draft = normalize_episode(script_to_draft(parse_script(reply).unwrap(), "pairs", "f")).unwrap();
        let lines = &draft.sections[0].lines;
        assert_eq!(lines[0].speaker, "", "the narrator is no one in particular");
        assert_eq!((lines[1].speaker.as_str(), lines[1].how.as_str()), ("Kasia", "surprised"));
        let cast = draft.podcast.unwrap().cast;
        let names: Vec<(&str, &str, &str)> =
            cast.iter().map(|m| (m.name.as_str(), m.gender.as_str(), m.character.as_str())).collect();
        assert_eq!(names, [("Kasia", "f", "curious"), ("Marek", "f", "")], "no one twice; unlisted speakers join");

        // The lyrics say who speaks.
        let clips: Vec<Vec<u8>> = (0..6).map(|_| clip(100)).collect();
        let episode = assemble(&draft.sections, &clips, &PodcastAudio::for_format("pairs")).unwrap();
        let speakers: Vec<Option<&str>> = episode.lines.iter().map(|l| l.speaker.as_deref()).collect();
        assert_eq!(speakers, [None, None, None, Some("Kasia"), None, Some("Marek")]);
    }

    #[test]
    fn hosts_discuss_articles_and_stories_have_characters() {
        assert_eq!(cast_of("", false), "hosts");
        assert_eq!(cast_of("", true), "characters");
        assert_eq!(cast_of("narrator", true), "narrator");
        assert!(cast_rules("narrator").contains("one narrator"));
    }

    #[test]
    fn news_picks_are_checked_and_the_reading_keeps_to_them() {
        let reply = r#"{"stories": [
            {"headlines": [3, 17, 4], "background": ["Sejm", "Warsaw"]},
            {"headlines": [3, 99], "background": []},
            {"headlines": [5], "background": ["Sejm", "A", "B", "C", "D"]}
        ]}"#;
        let (stories, background) = parse_picks(reply, 20);
        assert_eq!(stories, vec![vec![3, 17], vec![5]], "in range, once each, two at most");
        assert_eq!(background, ["Sejm", "Warsaw", "A", "B", "C"]);
        assert_eq!(parse_picks("nonsense", 20), (Vec::new(), Vec::new()));

        let story = vec![news::Headline {
            title: "Nowy most w Krakowie".into(),
            outlet: "pap.pl".into(),
            date: "2026-11-16".into(),
            summary: "Most zostanie otwarty w 2030 roku.".into(),
            ..Default::default()
        }];
        let text = news_reading(&[story], &[], "2026-11-16");
        assert!(text.contains("NEWS episode for 2026-11-16"));
        assert!(text.contains("“Nowy most w Krakowie” (pap.pl, 2026-11-16)") && text.contains("Summary: Most zostanie otwarty w 2030 roku."));
        assert!(!text.contains("Background"), "no background without articles");
    }

    #[test]
    fn clips_must_share_one_format() {
        let sections = vec![SongSection { kind: "verse".into(), lines: vec![line("Hello", "Cześć")] }];
        let clips = vec![clip(10), ai::wav_from_pcm(&[0u8; 20], 2000, 1)];
        assert!(assemble(&sections, &clips, &PodcastAudio::default()).is_err());
        assert!(assemble(&sections, &clips[..1], &PodcastAudio::default()).is_err(), "a clip missing");
    }

    #[test]
    fn scripts_become_tidy_episodes() {
        let reply = r#"Sure! {"title": "The bridge", "titlePl": "Kopalnia", "about": "A mine.",
            "parts": [{"lines": [{"en": " There is a mine. ", "pl": "Jest kopalnia."}, {"en": "", "pl": ""}]}, {"lines": []}]}"#;
        let draft = normalize_episode(script_to_draft(parse_script(reply).unwrap(), "immersion", "")).unwrap();
        assert_eq!(draft.kind, "podcast");
        assert_eq!(draft.style, "immersion");
        assert_eq!(draft.vocal_gender, "f");
        assert_eq!(draft.chorus_pl, "Kopalnia");
        assert_eq!(draft.sections.len(), 1, "empty parts are dropped");
        assert_eq!(draft.sections[0].lines, vec![line("There is a mine.", "Jest kopalnia.")]);
        assert_eq!(draft.podcast, Some(PodcastAudio::for_format("immersion")));
        assert!(!PodcastAudio::for_format("immersion").english, "immersion says no English");
        assert!(normalize_episode(SongDraft::default()).is_err());
    }

    #[test]
    fn audio_settings_are_kept_sensible() {
        let wild = PodcastAudio { pause: 99.0, repeats: 0, slow: true, voices: "many".into(), english: false, cast: Vec::new() }.clean();
        assert_eq!(wild, PodcastAudio { pause: 6.0, repeats: 1, slow: true, voices: "two".into(), english: false, cast: Vec::new() });
        // Episodes from before the setting keep their English.
        let old: PodcastAudio = serde_json::from_str(r#"{"pause": 2.0, "repeats": 2, "slow": true, "voices": "two"}"#).unwrap();
        assert!(old.english);
    }

    #[test]
    fn articles_become_sources_and_the_reading() {
        let article = |pageid: i64, title: &str, content: &str| wiki::Article {
            lang: "en".into(),
            title: title.into(),
            url: format!("https://en.wikipedia.org/wiki/{title}"),
            pageid,
            revid: 1,
            summary: String::new(),
            content: content.into(),
            thumbnail: None,
            polish_title: Some("Kopalnia Soli Wieliczka".into()),
            html: None,
        };
        let articles = vec![article(1, "Wieliczka Salt Mine", &"b".repeat(20_000)), article(2, "Kraków", "Kraków is a city.")];
        let sources = sources_of(&articles);
        assert_eq!(sources.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["Wieliczka Salt Mine", "Kraków"]);
        assert!(sources.iter().all(|s| s.lang == "en"), "the Polish versions are not sources of their own");

        let text = reading(&articles, false);
        assert!(text.contains("### “Wieliczka Salt Mine”") && text.contains("Kraków is a city."));
        assert!(text.len() < READING_LIMIT + 1_000, "the articles share the reading limit");
        assert!(reading(&articles, true).contains("STORY"), "a story ignores articles");
        assert!(reading(&[], false).contains("No article"));
    }
}

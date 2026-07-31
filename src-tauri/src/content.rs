//! The whole curriculum is compiled into the binary, so a fresh install works
//! offline and the seed can never drift from the code that reads it.

use serde::Deserialize;

pub struct TopicFiles {
    pub slug: &'static str,
    pub words: &'static str,
    pub sentences: &'static str,
    pub conversation: &'static str,
}

macro_rules! topic {
    ($slug:literal, $file:literal) => {
        TopicFiles {
            slug: $slug,
            words: include_str!(concat!("../content/words/", $file, ".tsv")),
            sentences: include_str!(concat!("../content/sentences/", $file, ".tsv")),
            conversation: include_str!(concat!("../content/conversations/", $file, ".json")),
        }
    };
}

pub const TOPICS: &[TopicFiles] = &[
    topic!("basics", "01-basics"),
    topic!("family", "02-family"),
    topic!("food", "03-food"),
    topic!("shopping", "04-shopping"),
    topic!("travel", "05-travel"),
    topic!("home", "06-home"),
    topic!("work", "07-work"),
    topic!("health", "08-health"),
    topic!("time-nature", "09-time-nature"),
    topic!("leisure", "10-leisure"),
];

#[derive(Debug, Deserialize)]
pub struct Conversation {
    pub slug: String,
    pub title_pl: String,
    pub title_en: String,
    pub title_da: String,
    pub summary_en: String,
    pub summary_da: String,
    /// Speaker name to "f" or "m", so each line can be read in a matching voice.
    pub speakers: std::collections::HashMap<String, String>,
    pub lines: Vec<ConversationLine>,
}

#[derive(Debug, Deserialize)]
pub struct ConversationLine {
    pub speaker: String,
    pub pl: String,
    pub en: String,
    pub da: String,
}

pub struct WordRow<'a> {
    pub pl: &'a str,
    pub en: &'a str,
    pub da: &'a str,
    pub pos: &'a str,
}

pub struct SentenceRow<'a> {
    pub pl: &'a str,
    pub en: &'a str,
    pub da: &'a str,
}

/// Splits a TSV body into rows, skipping comments and blank lines. Rows with the
/// wrong number of columns are dropped rather than failing the whole seed — a
/// single malformed line should not leave a user with an empty app.
fn rows(body: &str, columns: usize) -> impl Iterator<Item = Vec<&str>> {
    body.lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .filter(move |cells| cells.len() == columns)
}

pub fn parse_words(body: &str) -> Vec<WordRow<'_>> {
    rows(body, 4)
        .map(|c| WordRow {
            pl: c[0],
            en: c[1],
            da: c[2],
            pos: c[3],
        })
        .collect()
}

pub fn parse_sentences(body: &str) -> Vec<SentenceRow<'_>> {
    rows(body, 3)
        .map(|c| SentenceRow {
            pl: c[0],
            en: c[1],
            da: c[2],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_topic_parses_and_is_substantial() {
        assert_eq!(TOPICS.len(), 10);
        let mut total_words = 0;
        for topic in TOPICS {
            let words = parse_words(topic.words);
            let sentences = parse_sentences(topic.sentences);
            let convo: Conversation = serde_json::from_str(topic.conversation)
                .unwrap_or_else(|e| panic!("{} conversation: {e}", topic.slug));

            assert!(words.len() >= 190, "{} has {} words", topic.slug, words.len());
            assert_eq!(sentences.len(), 20, "{}", topic.slug);
            assert!(convo.lines.len() >= 10, "{}", topic.slug);
            assert_eq!(convo.slug, topic.slug);

            // Every speaker needs a gender, or their lines get the wrong voice.
            for line in &convo.lines {
                let gender = convo.speakers.get(&line.speaker).unwrap_or_else(|| {
                    panic!("{}: speaker '{}' has no gender", topic.slug, line.speaker)
                });
                assert!(
                    gender == "f" || gender == "m",
                    "{}: '{}' has gender '{gender}'",
                    topic.slug,
                    line.speaker
                );
            }
            total_words += words.len();
        }
        assert!(total_words >= 2000, "only {total_words} words");
    }

    #[test]
    fn headwords_are_globally_unique() {
        let mut seen = std::collections::HashSet::new();
        for topic in TOPICS {
            for word in parse_words(topic.words) {
                assert!(seen.insert(word.pl.to_string()), "duplicate: {}", word.pl);
            }
        }
    }
}

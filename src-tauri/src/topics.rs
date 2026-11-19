//! Topics: what words, sentences and lessons are about — food and its
//! drinks, fruit and meals; travel and its transport and accommodation;
//! time, family, the body, work, home… A curated ontology kept here, like
//! the grammar tags (grammar.rs): domains, each with its topics, every id
//! derived from its key (DATA.md), the same on every device, and nothing is
//! written for the list itself.
//!
//! Words, sentences and lessons are linked to topics (`word_topics`,
//! `sentence_topics`, `lesson_topics`). The tutor tags what it makes
//! (`topics` on a word, sentence or lesson in its tools); older content is
//! tagged only when the learner asks it to ("Tag the topics in your
//! courses" on the Topics page) — never behind their back (PRINCIPLES.md #4).
//! A topic's numbers count toward its domain too (drinks are food).

use std::collections::HashMap;

use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::db::Conn;
use crate::error::{Error, Result};
#[allow(unused_imports)]
use crate::ids::*;
use crate::schema::{lesson_topics, sentence_topics, word_topics};

/// One topic: its key (what the tutor writes: "food.drinks", or a domain's
/// own "food"), its English name, and its domain's key.
#[derive(Debug)]
pub struct Topic {
    pub key: &'static str,
    pub name: &'static str,
    pub domain: &'static str,
}

const fn t(key: &'static str, name: &'static str, domain: &'static str) -> Topic {
    Topic { key, name, domain }
}

/// Every topic, each domain first and then its topics, in the order listed.
pub const TOPICS: &[Topic] = &[
    t("food", "Food and drink", "food"),
    t("food.drinks", "Drinks", "food"),
    t("food.fruit-veg", "Fruit and vegetables", "food"),
    t("food.meals", "Meals", "food"),
    t("food.cooking", "Cooking", "food"),
    t("food.restaurant", "Cafés and restaurants", "food"),
    t("food.sweets", "Sweets and snacks", "food"),
    t("travel", "Travel", "travel"),
    t("travel.transport", "Transport", "travel"),
    t("travel.accommodation", "Accommodation", "travel"),
    t("travel.directions", "Directions", "travel"),
    t("travel.airport-station", "Airports and stations", "travel"),
    t("travel.sightseeing", "Sightseeing", "travel"),
    t("travel.holidays", "Holidays and trips", "travel"),
    t("time", "Time", "time"),
    t("time.clock", "Telling the time", "time"),
    t("time.days-dates", "Days, months and dates", "time"),
    t("time.seasons", "Seasons", "time"),
    t("time.routine", "Daily routine", "time"),
    t("time.frequency", "How often", "time"),
    t("family", "Family", "family"),
    t("family.relatives", "Relatives", "family"),
    t("family.relationships", "Relationships", "family"),
    t("family.celebrations", "Family occasions", "family"),
    t("people", "People", "people"),
    t("people.appearance", "Appearance", "people"),
    t("people.character", "Character", "people"),
    t("people.age", "Age", "people"),
    t("people.origin", "Countries and nationalities", "people"),
    t("people.introductions", "Introductions", "people"),
    t("body", "The body", "body"),
    t("body.parts", "Parts of the body", "body"),
    t("body.senses", "The senses", "body"),
    t("health", "Health", "health"),
    t("health.illness", "Illness and symptoms", "health"),
    t("health.doctor", "At the doctor's", "health"),
    t("health.pharmacy", "Pharmacy", "health"),
    t("health.fitness", "Fitness", "health"),
    t("home", "Home", "home"),
    t("home.rooms", "Rooms", "home"),
    t("home.furniture", "Furniture and things", "home"),
    t("home.chores", "Housework", "home"),
    t("home.renting", "Renting and moving", "home"),
    t("work", "Work", "work"),
    t("work.jobs", "Jobs", "work"),
    t("work.office", "At the office", "work"),
    t("work.meetings", "Meetings and calls", "work"),
    t("work.money", "Money and banking", "work"),
    t("shopping", "Shopping", "shopping"),
    t("shopping.clothes", "Clothes", "shopping"),
    t("shopping.groceries", "Groceries", "shopping"),
    t("shopping.shops", "Shops", "shopping"),
    t("shopping.prices", "Prices and paying", "shopping"),
    t("school", "School and study", "school"),
    t("school.subjects", "Subjects", "school"),
    t("school.classroom", "The classroom", "school"),
    t("school.university", "University", "school"),
    t("school.languages", "Learning languages", "school"),
    t("nature", "Nature", "nature"),
    t("nature.animals", "Animals", "nature"),
    t("nature.plants", "Plants", "nature"),
    t("nature.landscape", "Landscape", "nature"),
    t("nature.environment", "The environment", "nature"),
    t("weather", "Weather", "weather"),
    t("weather.conditions", "Sun, rain and snow", "weather"),
    t("weather.temperature", "Hot and cold", "weather"),
    t("emotions", "Feelings", "emotions"),
    t("emotions.feelings", "Moods and feelings", "emotions"),
    t("emotions.opinions", "Likes and opinions", "emotions"),
    t("city", "The city", "city"),
    t("city.places", "Places in town", "city"),
    t("city.services", "Post office, bank, offices", "city"),
    t("city.traffic", "Streets and traffic", "city"),
    t("leisure", "Free time", "leisure"),
    t("leisure.sport", "Sport", "leisure"),
    t("leisure.music", "Music", "leisure"),
    t("leisure.film-tv", "Film and TV", "leisure"),
    t("leisure.reading", "Books and reading", "leisure"),
    t("leisure.hobbies", "Hobbies", "leisure"),
    t("leisure.going-out", "Going out", "leisure"),
    t("communication", "Talking to people", "communication"),
    t("communication.greetings", "Greetings and goodbyes", "communication"),
    t("communication.politeness", "Please, thanks, sorry", "communication"),
    t("communication.small-talk", "Small talk", "communication"),
    t("communication.phone-internet", "Phone and internet", "communication"),
    t("culture", "Culture", "culture"),
    t("culture.traditions", "Traditions and holidays", "culture"),
    t("culture.history", "History", "culture"),
    t("culture.religion", "Religion", "culture"),
    t("culture.art", "Art", "culture"),
    t("basics", "Basics", "basics"),
    t("basics.numbers", "Numbers", "basics"),
    t("basics.colours", "Colours", "basics"),
    t("basics.sizes-shapes", "Sizes and shapes", "basics"),
    t("basics.quantities", "Amounts and measures", "basics"),
    t("technology", "Technology", "technology"),
    t("technology.computers", "Computers", "technology"),
    t("technology.devices", "Phones and devices", "technology"),
];

/// A topic's id: derived from its key, so it is the same everywhere.
pub fn id_of(key: &str) -> TopicId {
    TopicId::derived("topic", key)
}

/// Every topic's key, for the tutor's tool schemas (an enum).
pub fn keys() -> Vec<&'static str> {
    TOPICS.iter().map(|t| t.key).collect()
}

pub fn by_key(key: &str) -> Option<&'static Topic> {
    let key = key.trim().to_lowercase();
    TOPICS.iter().find(|t| t.key == key)
}

pub fn by_id(id: TopicId) -> Option<&'static Topic> {
    TOPICS.iter().find(|t| id_of(t.key) == id)
}

/// A topic and its domain: itself for a domain, its domain for a topic.
pub fn with_domain(id: TopicId) -> Vec<TopicId> {
    match by_id(id) {
        Some(t) if t.key != t.domain => vec![id, id_of(t.domain)],
        _ => vec![id],
    }
}

/// A topic as the screens and the tutor see it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TopicInfo {
    pub id: TopicId,
    pub key: String,
    pub name: String,
    pub domain: String,
    pub domain_name: String,
    /// A domain rather than one of its topics.
    pub is_domain: bool,
}

impl From<&Topic> for TopicInfo {
    fn from(t: &Topic) -> Self {
        TopicInfo {
            id: id_of(t.key),
            key: t.key.into(),
            name: t.name.into(),
            domain: t.domain.into(),
            domain_name: by_key(t.domain).map(|d| d.name).unwrap_or_default().into(),
            is_domain: t.key == t.domain,
        }
    }
}

/// The topics given, checked: every key must be one of the list (the error
/// names the unknown ones, so the tutor can correct itself); duplicates are
/// dropped. At most four: a topic says what the thing is about.
pub fn validate(keys: &[String]) -> Result<Vec<&'static Topic>> {
    let mut kept: Vec<&'static Topic> = Vec::new();
    let mut unknown: Vec<String> = Vec::new();
    for key in keys {
        match by_key(key) {
            Some(topic) if !kept.iter().any(|k| k.key == topic.key) => kept.push(topic),
            Some(_) => {}
            None => unknown.push(key.clone()),
        }
    }
    if !unknown.is_empty() {
        return Err(Error::msg(format!(
            "Unknown topics: {}. Use only keys from the list (e.g. \"food.drinks\", \"travel.transport\", \"time\").",
            unknown.join(", ")
        )));
    }
    kept.truncate(4);
    Ok(kept)
}

/// Gives a word its topics (replacing any it had).
pub fn set_word(conn: &mut Conn, word_id: WordId, topics: &[&Topic]) -> Result<()> {
    diesel::delete(word_topics::table.filter(word_topics::word_id.eq(word_id))).execute(conn)?;
    for t in topics {
        diesel::insert_or_ignore_into(word_topics::table)
            .values((word_topics::word_id.eq(word_id), word_topics::topic.eq(id_of(t.key))))
            .execute(conn)?;
    }
    Ok(())
}

/// Gives a sentence its topics (replacing any it had).
pub fn set_sentence(conn: &mut Conn, sentence_id: SentenceId, topics: &[&Topic]) -> Result<()> {
    diesel::delete(sentence_topics::table.filter(sentence_topics::sentence_id.eq(sentence_id))).execute(conn)?;
    for t in topics {
        diesel::insert_or_ignore_into(sentence_topics::table)
            .values((sentence_topics::sentence_id.eq(sentence_id), sentence_topics::topic.eq(id_of(t.key))))
            .execute(conn)?;
    }
    Ok(())
}

/// Gives a lesson its topics (replacing any it had).
pub fn set_lesson(conn: &mut Conn, lesson_id: LessonId, topics: &[&Topic]) -> Result<()> {
    diesel::delete(lesson_topics::table.filter(lesson_topics::lesson_id.eq(lesson_id))).execute(conn)?;
    for t in topics {
        diesel::insert_or_ignore_into(lesson_topics::table)
            .values((lesson_topics::lesson_id.eq(lesson_id), lesson_topics::topic.eq(id_of(t.key))))
            .execute(conn)?;
    }
    Ok(())
}

/// Every tagged word's, sentence's and lesson's topics: id (as text) →
/// topic ids, as tagged (without their domains).
pub fn all_links(conn: &mut Conn) -> Result<HashMap<String, Vec<TopicId>>> {
    let mut out: HashMap<String, Vec<TopicId>> = HashMap::new();
    for (word, topic) in word_topics::table.select((word_topics::word_id, word_topics::topic)).load::<(WordId, TopicId)>(conn)? {
        out.entry(word.to_string()).or_default().push(topic);
    }
    for (sentence, topic) in
        sentence_topics::table.select((sentence_topics::sentence_id, sentence_topics::topic)).load::<(SentenceId, TopicId)>(conn)?
    {
        out.entry(sentence.to_string()).or_default().push(topic);
    }
    for (lesson, topic) in lesson_topics::table.select((lesson_topics::lesson_id, lesson_topics::topic)).load::<(LessonId, TopicId)>(conn)? {
        out.entry(lesson.to_string()).or_default().push(topic);
    }
    Ok(out)
}

/// One thing the tutor tags (`tag_topics`): a word, sentence or lesson by id.
#[derive(Debug, Deserialize)]
pub struct TopicItem {
    pub id: String,
    #[serde(default)]
    pub topics: Vec<String>,
}

/// Tags existing words, sentences and lessons (the tutor, asked by the
/// learner). Every id and topic is checked; one bad one fails the whole
/// call (the tool's transaction), saying which. Returns how many of each.
pub fn tag_items(conn: &mut Conn, items: &[TopicItem]) -> Result<(usize, usize, usize)> {
    if items.len() > 200 {
        return Err(Error::msg("At most 200 items per call: tag the rest in another call."));
    }
    let (mut words, mut sentences, mut lessons) = (0, 0, 0);
    for item in items {
        let topics = validate(&item.topics)?;
        let id = item.id.trim();
        match id.split_once('_').map(|(p, _)| p) {
            Some("wd") => {
                let word = WordId::parse(id).map_err(|e| Error::msg(e.to_string()))?;
                if crate::schema::words::table.find(word).count().get_result::<i64>(conn)? == 0 {
                    return Err(Error::msg(format!("No word {id}.")));
                }
                set_word(conn, word, &topics)?;
                words += 1;
            }
            Some("sn") => {
                let sentence = SentenceId::parse(id).map_err(|e| Error::msg(e.to_string()))?;
                if crate::schema::sentences::table.find(sentence).count().get_result::<i64>(conn)? == 0 {
                    return Err(Error::msg(format!("No sentence {id}.")));
                }
                set_sentence(conn, sentence, &topics)?;
                sentences += 1;
            }
            Some("ls") => {
                let lesson = LessonId::parse(id).map_err(|e| Error::msg(e.to_string()))?;
                if crate::schema::lessons::table.find(lesson).count().get_result::<i64>(conn)? == 0 {
                    return Err(Error::msg(format!("No lesson {id}.")));
                }
                set_lesson(conn, lesson, &topics)?;
                lessons += 1;
            }
            _ => return Err(Error::msg(format!("{id}: give a word (wd_…), sentence (sn_…) or lesson (ls_…) id."))),
        }
    }
    Ok((words, sentences, lessons))
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    n: i64,
}

/// Words, sentences and lessons in open courses with no topic yet: what the
/// Topics page's button offers to do.
pub fn untagged_count(conn: &mut Conn) -> Result<i64> {
    Ok(diesel::sql_query(
        "SELECT (SELECT count(DISTINCT w.id) FROM words w
                   JOIN course_lessons cl ON cl.lesson_id = w.lesson_id
                   JOIN courses c ON c.id = cl.course_id AND c.closed_at IS NULL
                  WHERE NOT EXISTS (SELECT 1 FROM word_topics g WHERE g.word_id = w.id))
              + (SELECT count(DISTINCT s.id) FROM sentences s
                   JOIN course_lessons cl ON cl.lesson_id = s.lesson_id
                   JOIN courses c ON c.id = cl.course_id AND c.closed_at IS NULL
                  WHERE NOT EXISTS (SELECT 1 FROM sentence_topics g WHERE g.sentence_id = s.id))
              + (SELECT count(DISTINCT l.id) FROM lessons l
                   JOIN course_lessons cl ON cl.lesson_id = l.id
                   JOIN courses c ON c.id = cl.course_id AND c.closed_at IS NULL
                  WHERE NOT EXISTS (SELECT 1 FROM lesson_topics g WHERE g.lesson_id = l.id)) AS n",
    )
    .get_result::<Count>(conn)?
    .n)
}

/// Untagged lessons, words and sentences, a page of them, for the tutor:
/// (id, Polish or title, English, course).
pub fn untagged(conn: &mut Conn, course_id: Option<CourseId>, limit: i64) -> Result<Vec<(String, String, String, String)>> {
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = diesel::sql_types::Text)]
        id: String,
        #[diesel(sql_type = diesel::sql_types::Text)]
        pl: String,
        #[diesel(sql_type = diesel::sql_types::Text)]
        en: String,
        #[diesel(sql_type = diesel::sql_types::Text)]
        course: String,
    }
    let course = course_id.map(|c| c.to_string()).unwrap_or_default();
    let rows: Vec<Row> = diesel::sql_query(
        "SELECT DISTINCT l.id AS id, l.title AS pl, l.summary AS en, c.title_en AS course FROM lessons l
           JOIN course_lessons cl ON cl.lesson_id = l.id
           JOIN courses c ON c.id = cl.course_id AND c.closed_at IS NULL
          WHERE NOT EXISTS (SELECT 1 FROM lesson_topics g WHERE g.lesson_id = l.id) AND (?1 = '' OR c.id = ?1)
         UNION
         SELECT DISTINCT w.id, w.pl, w.en, c.title_en FROM words w
           JOIN course_lessons cl ON cl.lesson_id = w.lesson_id
           JOIN courses c ON c.id = cl.course_id AND c.closed_at IS NULL
          WHERE NOT EXISTS (SELECT 1 FROM word_topics g WHERE g.word_id = w.id) AND (?1 = '' OR c.id = ?1)
         UNION
         SELECT DISTINCT s.id, s.pl, s.en, c.title_en FROM sentences s
           JOIN course_lessons cl ON cl.lesson_id = s.lesson_id
           JOIN courses c ON c.id = cl.course_id AND c.closed_at IS NULL
          WHERE NOT EXISTS (SELECT 1 FROM sentence_topics g WHERE g.sentence_id = s.id) AND (?1 = '' OR c.id = ?1)
         ORDER BY course, id LIMIT ?2",
    )
    .bind::<diesel::sql_types::Text, _>(course)
    .bind::<diesel::sql_types::BigInt, _>(limit.clamp(1, 200))
    .load(conn)?;
    Ok(rows.into_iter().map(|r| (r.id, r.pl, r.en, r.course)).collect())
}

/// The ontology as the tutor's prompt lists it: domain: its topics' keys.
pub fn for_tutor() -> String {
    let mut out = String::new();
    for domain in TOPICS.iter().filter(|t| t.key == t.domain) {
        let subs: Vec<&str> = TOPICS.iter().filter(|t| t.domain == domain.key && t.key != t.domain).map(|t| t.key).collect();
        out.push_str(&format!("\n  {}: {}", domain.key, subs.join(", ")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ontology_is_well_formed() {
        assert!((60..=110).contains(&TOPICS.len()), "{}", TOPICS.len());
        let keys: std::collections::HashSet<_> = TOPICS.iter().map(|t| t.key).collect();
        assert_eq!(keys.len(), TOPICS.len(), "no two topics share a key");
        for t in TOPICS {
            assert!(t.key.chars().all(|c| c.is_ascii_lowercase() || c == '-' || c == '.'), "{}", t.key);
            let domain = by_key(t.domain).expect("every domain is a topic");
            assert_eq!(domain.domain, domain.key, "{} is a domain", domain.key);
            if t.key != t.domain {
                assert!(t.key.starts_with(&format!("{}.", t.domain)), "{} sits under its domain", t.key);
            }
            assert_eq!(by_id(id_of(t.key)).map(|x| x.key), Some(t.key));
        }
        assert!(id_of("food.drinks").to_string().starts_with("tp_"));
        assert_eq!(with_domain(id_of("food.drinks")), vec![id_of("food.drinks"), id_of("food")]);
        assert_eq!(with_domain(id_of("food")), vec![id_of("food")]);
    }

    #[test]
    fn unknown_topics_are_named_and_duplicates_dropped() {
        let ok = validate(&["Food.Drinks".into(), "time".into(), "food.drinks".into()]).unwrap();
        assert_eq!(ok.iter().map(|t| t.key).collect::<Vec<_>>(), ["food.drinks", "time"]);
        let err = validate(&["time".into(), "food.beer".into()]).unwrap_err().to_string();
        assert!(err.contains("food.beer"), "{err}");
    }
}

//! The model benchmark: a small suite of the jobs this app gives a model,
//! run against one model with the learner's own key, scored by rules — no
//! model grades another.
//!
//! Three parts, each 0–100:
//! - **Polish**: short items with known answers — cases, conjugations,
//!   numbers, fixing a planted mistake, a translation.
//! - **Tools**: the tutor's real tools offered with requests to act on
//!   (nothing is made): is the call valid, the counts right, the Polish
//!   spelled right — and a plain question must not trigger a tool.
//! - **Rules**: what the prompts ask — answer in the learner's language,
//!   keep it short, give Polish a player, stay in Polish as the partner.
//!
//! Each run is kept with its time, cost and every item's outcome.

use diesel::prelude::*;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{Emitter, State};

use crate::ai::{self, Wire};
use crate::db::{self, Conn, Db};
use crate::error::{Error, Result};
use crate::schema::bench_runs;

/// What one item checks.
enum Check {
    /// The reply, cleaned up, is one of these (diacritics count).
    Exactly(&'static [&'static str]),
    /// The reply contains one of these.
    Contains(&'static [&'static str]),
    /// A tool-use item: scored from the call (see [`score_tools`]).
    Tool(ToolTask),
    /// An instruction-following item (see [`score_rule`]).
    Rule(RuleTask),
}

#[derive(Clone, Copy)]
enum ToolTask {
    Course,
    Writing,
    NoTool,
}

#[derive(Clone, Copy)]
enum RuleTask {
    Danish,
    ShortWithPlayer,
    PartnerStaysPolish,
    WordChatOpening,
}

struct Item {
    part: &'static str,
    /// Shown with the result.
    name: &'static str,
    prompt: &'static str,
    check: Check,
}

const FILL: &str = "Fill in the blank with the correct Polish form of the word in brackets. Reply \
with one line and nothing else, exactly in this form:\nANSWER: <the missing word>";
const FIX: &str = "The Polish sentence has one grammar mistake. Reply with one line and nothing \
else, exactly in this form:\nANSWER: <the corrected sentence>";

fn items() -> Vec<Item> {
    use Check::*;
    let polish = |name, prompt, check| Item { part: "polish", name, prompt, check };
    vec![
        polish("Locative of a city", "Mieszkam w ___ (Kraków).", Exactly(&["krakowie"])),
        polish("Genitive after do", "Idę do ___ (szkoła).", Exactly(&["szkoły"])),
        polish("Genitive after negation", "Nie mam ___ (czas).", Exactly(&["czasu"])),
        polish("Instrumental after z", "Rozmawiam z ___ (mama).", Exactly(&["mamą"])),
        polish("Accusative, animate", "Widzę ___ (kot).", Exactly(&["kota"])),
        polish("być, 1st person", "Ja ___ (być) studentem.", Exactly(&["jestem"])),
        polish("mieć, 1st person plural", "My ___ (mieć) psa.", Exactly(&["mamy"])),
        polish("iść, 3rd person plural", "Oni ___ (iść) do kina.", Exactly(&["idą"])),
        polish("Past tense, feminine", "Wczoraj (ja, kobieta) ___ (czytać) książkę.", Exactly(&["czytałam"])),
        polish("Perfective future", "Jutro ___ (ja, zrobić) zakupy.", Exactly(&["zrobię"])),
        polish("Number 2–4", "Mam dwa ___ (kot).", Exactly(&["koty"])),
        polish("Number 5+", "Mam pięć ___ (kot).", Exactly(&["kotów"])),
        polish("Possessive agreement", "To jest ___ (mój) siostra.", Exactly(&["moja"])),
        polish("Fix: dwa/dwie", "Ona ma dwa córki.", Contains(&["dwie córki"])),
        polish("Fix: na + accusative", "Idę na poczcie.", Contains(&["na pocztę"])),
        polish(
            "Translate a request",
            "Translate into Polish, reply with only the Polish: \"I would like a coffee, please.\"",
            Contains(&["kawę"]),
        ),
        Item { part: "tools", name: "Make a course", prompt: "Make me a course about the weather with exactly 10 words and 5 sentences.", check: Tool(ToolTask::Course) },
        Item { part: "tools", name: "Make a writing block", prompt: "Make a writing block where I order food in a restaurant, with exactly 3 goals.", check: Tool(ToolTask::Writing) },
        Item { part: "tools", name: "A question is not an order", prompt: "What does \"dziękuję\" mean?", check: Tool(ToolTask::NoTool) },
        Item { part: "rules", name: "Answers in Danish", prompt: "Hvad er forskellen på \"ty\" og \"pan\"?", check: Rule(RuleTask::Danish) },
        Item { part: "rules", name: "Short, with a player", prompt: "How do I say \"thank you\" in Polish?", check: Rule(RuleTask::ShortWithPlayer) },
        Item { part: "rules", name: "Partner stays in Polish", prompt: "Hello! I live on the second floor, next to you.", check: Rule(RuleTask::PartnerStaysPolish) },
        Item { part: "rules", name: "Word chat opens briefly", prompt: "Let's talk about “kot”.", check: Rule(RuleTask::WordChatOpening) },
    ]
}

/// The messages an item sends.
fn messages(item: &Item) -> Vec<Wire> {
    match &item.check {
        Check::Exactly(_) => vec![Wire::system(FILL), Wire::user(item.prompt)],
        Check::Contains(_) if item.name.starts_with("Fix") => vec![Wire::system(FIX), Wire::user(item.prompt)],
        Check::Contains(_) => vec![Wire::user(item.prompt)],
        Check::Tool(_) => vec![Wire::system(crate::tutor::RULES), Wire::user(item.prompt)],
        Check::Rule(RuleTask::PartnerStaysPolish) => vec![
            Wire::system(PARTNER),
            Wire::assistant("Dzień dobry! Jestem Ewa, mieszkam pod trójką. Pan tu nowy?"),
            Wire::user(item.prompt),
        ],
        Check::Rule(RuleTask::WordChatOpening) => vec![
            Wire::system(ai::with_say_player(format!(
                "{}\n\n{}",
                crate::tutor::RULES,
                crate::tutor::word_chat_rules("kot — cat")
            ))),
            Wire::user(item.prompt),
        ],
        Check::Rule(_) => vec![Wire::system(ai::with_say_player(crate::tutor::RULES.to_string())), Wire::user(item.prompt)],
    }
}

/// The writing partner's core rules, as the app gives them.
const PARTNER: &str = "You play Pani Ewa, the learner's new neighbour, meeting them on the stairs. \
This is a role-play in writing, in Polish only, at level A1: very short, simple sentences.
- Stay in character. Write only in Polish. Never translate, never explain grammar, never switch \
  to English.
- You understand only Polish. If the learner writes in English or any other language, you don't \
  understand it: say so in Polish, in character (\"Przepraszam, nie rozumiem…\") and wait for Polish.";

/// One item's outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemResult {
    pub part: String,
    pub name: String,
    /// 0–1.
    pub score: f64,
    /// What went wrong, when something did.
    pub note: String,
    /// What the model said (shortened), to judge for yourself.
    pub answer: String,
}

/// A run, as kept.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchRun {
    pub id: i64,
    pub model: String,
    pub created_at: i64,
    pub score: i64,
    pub polish: i64,
    pub tools: i64,
    pub rules: i64,
    /// Average seconds per reply.
    pub seconds: f64,
    pub cost_usd: Option<f64>,
    pub items: Vec<ItemResult>,
}

/// Lowercase, without surrounding punctuation or quotes.
fn clean(answer: &str) -> String {
    answer
        .trim()
        .trim_matches(|c: char| c.is_ascii_punctuation() || "„”“\"'«» ".contains(c))
        .to_lowercase()
}

fn short(text: &str) -> String {
    // Shown as plain text: markdown marks would only be noise.
    let text = text.replace("**", "");
    let text = text.trim();
    if text.chars().count() > 300 {
        format!("{}…", text.chars().take(300).collect::<String>())
    } else {
        text.to_string()
    }
}

/// The text after the last "ANSWER:" line, markdown marks removed.
fn answer_line(reply: &str) -> Option<String> {
    reply
        .lines()
        .rev()
        .find_map(|line| {
            let plain = line.replace(['*', '_', '`'], "");
            let at = plain.to_uppercase().find("ANSWER:")?;
            Some(plain[at + "ANSWER:".len()..].trim().to_string())
        })
        .filter(|answer| !answer.is_empty())
}

/// The words a rambling reply put forward as its answer: its first line,
/// and anything in bold.
fn candidates(reply: &str) -> Vec<String> {
    let mut found: Vec<String> = reply.split("**").skip(1).step_by(2).map(clean).collect();
    if let Some(first) = reply.lines().find(|l| !l.trim().is_empty()) {
        found.push(clean(&first.replace(['*', '_', '`'], "")));
    }
    found
}

/// A known-answer item. The answer is read from the "ANSWER:" line it was
/// asked for; a right answer that had to be dug out of an explanation counts
/// half — the app needs models that do what they are told.
fn score_text(check: &Check, reply: &str) -> (f64, String) {
    let marked = answer_line(reply);
    match check {
        Check::Exactly(accepted) => {
            let right = |text: &str| accepted.contains(&clean(text).as_str());
            match &marked {
                Some(answer) if right(answer) => (1.0, String::new()),
                Some(_) => (0.0, format!("expected “{}”", accepted[0])),
                None if candidates(reply).iter().any(|c| accepted.contains(&c.as_str())) => {
                    (0.5, "right, but not in the answer form asked for".into())
                }
                None => (0.0, format!("expected “{}” (and no ANSWER: line)", accepted[0])),
            }
        }
        Check::Contains(any) => {
            let right = |text: &str| {
                let text = text.to_lowercase();
                any.iter().any(|a| text.contains(a))
            };
            match &marked {
                Some(answer) if right(answer) => (1.0, String::new()),
                Some(_) => (0.0, format!("expected it to contain “{}”", any[0])),
                // The translation item asks for plain Polish, no answer line.
                None if right(reply) && reply.lines().filter(|l| !l.trim().is_empty()).count() <= 2 => {
                    (1.0, String::new())
                }
                None if right(reply) => (0.5, "right, but buried in an explanation".into()),
                None => (0.0, format!("expected it to contain “{}”", any[0])),
            }
        }
        _ => (0.0, String::new()),
    }
}

/// Weather words a course about the weather should get right.
const WEATHER: &[&str] = &["deszcz", "słońce", "śnieg", "wiatr", "chmura", "pogoda", "burza", "mgła", "zimno", "ciepło", "upał", "mróz"];
const DIACRITICS: &str = "ąćęłńóśźż";

fn score_tools(task: ToolTask, turn: &ai::Turn) -> (f64, String) {
    let call = turn.tool_calls.first();
    match task {
        ToolTask::NoTool => match call {
            None if !turn.text.trim().is_empty() => (1.0, String::new()),
            None => (0.0, "no answer".into()),
            Some(c) => (0.0, format!("called {} for a plain question", c.function.name)),
        },
        ToolTask::Course | ToolTask::Writing => {
            let wanted = if matches!(task, ToolTask::Course) { "create_course" } else { "create_writing_block" };
            let Some(call) = turn.tool_calls.iter().find(|c| c.function.name == wanted) else {
                return (0.0, format!("did not call {wanted}"));
            };
            let Ok(args) = serde_json::from_str::<Value>(&call.function.arguments) else {
                return (0.1, "the call's arguments were not valid JSON".into());
            };
            let mut score: f64 = 0.3;
            let mut notes = Vec::new();
            if matches!(task, ToolTask::Writing) {
                let goals = args["goals"].as_array().map_or(0, Vec::len);
                if goals == 3 { score += 0.4 } else { notes.push(format!("{goals} goals, not 3")) }
                if args["scenario"].as_str().is_some_and(|s| s.len() > 20) { score += 0.3 } else { notes.push("no scenario".into()) }
            } else {
                let words = args["words"].as_array().cloned().unwrap_or_default();
                let sentences = args["sentences"].as_array().map_or(0, Vec::len);
                if words.len() == 10 { score += 0.2 } else { notes.push(format!("{} words, not 10", words.len())) }
                if sentences == 5 { score += 0.15 } else { notes.push(format!("{sentences} sentences, not 5")) }
                let polish: Vec<String> = words.iter().filter_map(|w| w["pl"].as_str()).map(str::to_lowercase).collect();
                if polish.len() == words.len() && words.iter().all(|w| w["en"].as_str().is_some_and(|e| !e.is_empty())) {
                    score += 0.1
                } else {
                    notes.push("words missing their Polish or English".into())
                }
                if polish.iter().any(|w| w.chars().any(|c| DIACRITICS.contains(c))) { score += 0.1 } else { notes.push("no diacritics at all".into()) }
                let known = polish.iter().filter(|w| WEATHER.contains(&w.as_str())).count();
                if known >= 3 { score += 0.15 } else { notes.push(format!("only {known} of the common weather words, spelled right")) }
            }
            (score.min(1.0), notes.join("; "))
        }
    }
}

fn words(text: &str) -> usize {
    text.split_whitespace().count()
}

fn score_rule(task: RuleTask, answer: &str) -> (f64, String) {
    let lower = format!(" {} ", answer.to_lowercase());
    match task {
        RuleTask::Danish => {
            let danish = ["æ", "ø", "å", " det ", " og ", " er ", " du ", " ikke "].iter().filter(|m| lower.contains(*m)).count();
            let english = [" the ", " you ", " is ", " and "].iter().filter(|m| lower.contains(*m)).count();
            if danish >= 2 && danish > english { (1.0, String::new()) } else { (0.0, "did not answer in Danish".into()) }
        }
        RuleTask::ShortWithPlayer => {
            let mut score = 0.0;
            let mut notes = Vec::new();
            if lower.contains("dziękuję") { score += 0.4 } else { notes.push("no “dziękuję”".to_string()) }
            if lower.contains("[[say:") { score += 0.3 } else { notes.push("no [[say: …]] player".to_string()) }
            if words(answer) <= 120 { score += 0.3 } else { notes.push(format!("{} words — long for a short question", words(answer))) }
            (score, notes.join("; "))
        }
        RuleTask::PartnerStaysPolish => {
            let english = [" the ", " i ", " you ", " floor", " hello", " nice "].iter().filter(|m| lower.contains(*m)).count();
            let polish = answer.chars().any(|c| DIACRITICS.contains(c.to_ascii_lowercase())) || lower.contains(" nie ");
            match (english, polish) {
                (0, true) => (1.0, String::new()),
                (0, false) => (0.5, "no recognisable Polish".into()),
                _ => (0.0, "switched to English".into()),
            }
        }
        RuleTask::WordChatOpening => {
            let mut score = 0.0;
            let mut notes = Vec::new();
            if words(answer) <= 110 { score += 0.5 } else { notes.push(format!("{} words — a lecture, not a chat", words(answer))) }
            if answer.contains('?') { score += 0.3 } else { notes.push("doesn't hand the conversation back".to_string()) }
            if !answer.contains("\n#") && !answer.starts_with('#') { score += 0.2 } else { notes.push("uses headings".to_string()) }
            (score, notes.join("; "))
        }
    }
}

/// Progress through a run (`bench://progress`).
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    done: usize,
    total: usize,
}

type Outcome = (ItemResult, f64, Option<f64>, ai::Tokens);

/// Runs one item of the suite: its reply, score, time, cost and tokens.
async fn run_item(
    app: tauri::AppHandle,
    key: String,
    model: String,
    tools: Value,
    index: usize,
    done: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    total: usize,
) -> Outcome {
    let list = items();
    let item = &list[index];
    let started = std::time::Instant::now();
    let wire = messages(item);
    let (score, note, answer, cost, tokens) = match &item.check {
        Check::Tool(task) => match ai::stream_turn(&key, &model, &wire, Some(&tools), |_| {}).await {
            Ok(turn) => {
                let (score, note) = score_tools(*task, &turn);
                let answer = match turn.tool_calls.first() {
                    Some(call) => format!("{}({})", call.function.name, short(&call.function.arguments)),
                    None => short(&turn.text),
                };
                (score, note, answer, turn.cost, turn.tokens)
            }
            Err(error) => (0.0, error.to_string(), String::new(), None, ai::Tokens::default()),
        },
        check => match ai::complete(&key, &model, &wire).await {
            Ok(reply) => {
                let (score, note) = match check {
                    Check::Rule(task) => score_rule(*task, &reply.text),
                    _ => score_text(check, &reply.text),
                };
                (score, note, short(&reply.text), reply.cost, reply.tokens)
            }
            Err(error) => (0.0, error.to_string(), String::new(), None, ai::Tokens::default()),
        },
    };
    let seconds = started.elapsed().as_secs_f64();
    let n = done.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    let _ = app.emit("bench://progress", Progress { done: n, total });
    (ItemResult { part: item.part.into(), name: item.name.into(), score, note, answer }, seconds, cost, tokens)
}

/// Runs the suite against `model` and keeps the result.
#[tauri::command]
pub async fn bench_run(app: tauri::AppHandle, db: State<'_, Db>, model: String) -> Result<BenchRun> {
    let model = model.trim().to_string();
    if model.is_empty() {
        return Err(Error::msg("Pick a model to test."));
    }
    let key = ai::api_key(&mut db.lock())?;
    let _busy = crate::work::begin(&app, "Testing a model…");
    let tools = Value::Array(crate::tutor::tool_definitions().into_iter().map(|t| t.definition).collect());
    let list = items();
    let total = list.len();
    let done = std::sync::atomic::AtomicUsize::new(0);

    let done = std::sync::Arc::new(done);
    let outcomes: Vec<Outcome> = futures_util::stream::iter(0..total)
        .map(|index| run_item(app.clone(), key.clone(), model.clone(), tools.clone(), index, done.clone(), total))
        .buffer_unordered(4)
        .collect()
        .await;

    let part = |name: &str| -> i64 {
        let scores: Vec<f64> = outcomes.iter().filter(|o| o.0.part == name).map(|o| o.0.score).collect();
        if scores.is_empty() { 0 } else { (scores.iter().sum::<f64>() / scores.len() as f64 * 100.0).round() as i64 }
    };
    let (polish, tools_score, rules) = (part("polish"), part("tools"), part("rules"));
    let score = ((polish + tools_score + rules) as f64 / 3.0).round() as i64;
    let seconds = outcomes.iter().map(|o| o.1).sum::<f64>() / outcomes.len().max(1) as f64;
    let priced: Vec<f64> = outcomes.iter().filter_map(|o| o.2).collect();
    let cost_usd: Option<f64> = (!priced.is_empty()).then(|| priced.iter().sum());
    // Keep the suite's order, not the order answers came back in.
    let mut items_out: Vec<ItemResult> = outcomes.iter().map(|o| o.0.clone()).collect();
    items_out.sort_by_key(|r| list.iter().position(|i| i.name == r.name));

    let mut guard = db.lock();
    let conn = &mut *guard;
    for (_, _, cost, tokens) in &outcomes {
        let _ = crate::costs::record_tokens(conn, crate::costs::BENCHMARK, &model, *cost, "", *tokens);
    }
    let id = diesel::insert_into(bench_runs::table)
        .values((
            bench_runs::model.eq(&model),
            bench_runs::created_at.eq(db::now_secs()),
            bench_runs::score.eq(score),
            bench_runs::polish.eq(polish),
            bench_runs::tools.eq(tools_score),
            bench_runs::rules.eq(rules),
            bench_runs::seconds.eq(seconds),
            bench_runs::cost_usd.eq(cost_usd),
            bench_runs::items.eq(serde_json::to_string(&items_out)?),
        ))
        .returning(bench_runs::id)
        .get_result::<i64>(conn)?;
    load(conn, id)
}

type Row = (i64, String, i64, i64, i64, i64, i64, f64, Option<f64>, String);

fn from_row(row: Row) -> BenchRun {
    let (id, model, created_at, score, polish, tools, rules, seconds, cost_usd, items) = row;
    BenchRun {
        id,
        model,
        created_at,
        score,
        polish,
        tools,
        rules,
        seconds,
        cost_usd,
        items: serde_json::from_str(&items).unwrap_or_default(),
    }
}

fn columns() -> (
    bench_runs::id,
    bench_runs::model,
    bench_runs::created_at,
    bench_runs::score,
    bench_runs::polish,
    bench_runs::tools,
    bench_runs::rules,
    bench_runs::seconds,
    bench_runs::cost_usd,
    bench_runs::items,
) {
    (
        bench_runs::id,
        bench_runs::model,
        bench_runs::created_at,
        bench_runs::score,
        bench_runs::polish,
        bench_runs::tools,
        bench_runs::rules,
        bench_runs::seconds,
        bench_runs::cost_usd,
        bench_runs::items,
    )
}

fn load(conn: &mut Conn, id: i64) -> Result<BenchRun> {
    Ok(from_row(bench_runs::table.find(id).select(columns()).first::<Row>(conn)?))
}

/// Every run, newest first.
pub fn runs(conn: &mut Conn) -> Result<Vec<BenchRun>> {
    Ok(bench_runs::table
        .order(bench_runs::id.desc())
        .select(columns())
        .load::<Row>(conn)?
        .into_iter()
        .map(from_row)
        .collect())
}

#[tauri::command]
pub async fn bench_runs(db: State<'_, Db>) -> Result<Vec<BenchRun>> {
    runs(&mut db.lock())
}

#[tauri::command]
pub async fn bench_delete(db: State<'_, Db>, id: i64) -> Result<()> {
    diesel::delete(bench_runs::table.find(id)).execute(&mut *db.lock())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(text: &str, call: Option<(&str, &str)>) -> ai::Turn {
        ai::Turn {
            text: text.into(),
            tool_calls: call
                .map(|(name, args)| ai::ToolCall {
                    id: "c".into(),
                    kind: "function".into(),
                    function: ai::FunctionCall { name: name.into(), arguments: args.into() },
                })
                .into_iter()
                .collect(),
            cost: None,
            tokens: ai::Tokens::default(),
        }
    }

    #[test]
    fn answers_are_scored_by_rules() {
        assert_eq!(score_text(&Check::Exactly(&["krakowie"]), "ANSWER: Krakowie.").0, 1.0);
        assert_eq!(score_text(&Check::Exactly(&["krakowie"]), "**ANSWER:** **Krakowie**").0, 1.0);
        assert_eq!(score_text(&Check::Exactly(&["krakowie"]), "ANSWER: Kraków").0, 0.0);
        assert_eq!(score_text(&Check::Exactly(&["mamą"]), "ANSWER: mama").0, 0.0, "diacritics count");
        // Right, but in an explanation instead of the answer line: half.
        assert_eq!(score_text(&Check::Exactly(&["krakowie"]), "**Krakowie**. *Note: the locative…*").0, 0.5);
        assert_eq!(score_text(&Check::Exactly(&["czasu"]), "Nie mam **czasu**. However…").0, 0.5);
        assert_eq!(score_text(&Check::Contains(&["dwie córki"]), "ANSWER: Ona ma dwie córki.").0, 1.0);
        assert_eq!(score_text(&Check::Contains(&["kawę"]), "Poproszę kawę.").0, 1.0);

        assert_eq!(score_tools(ToolTask::NoTool, &turn("It means thank you.", None)).0, 1.0);
        assert_eq!(score_tools(ToolTask::NoTool, &turn("", Some(("create_course", "{}")))).0, 0.0);
        let words: Vec<String> = ["deszcz", "słońce", "śnieg", "wiatr", "chmura", "pogoda", "burza", "mgła", "zimno", "ciepło"]
            .iter()
            .map(|w| format!("{{\"pl\":\"{w}\",\"en\":\"x\"}}"))
            .collect();
        let args = format!("{{\"words\":[{}],\"sentences\":[1,2,3,4,5]}}", words.join(","));
        assert_eq!(score_tools(ToolTask::Course, &turn("", Some(("create_course", &args)))).0, 1.0);
        assert!(score_tools(ToolTask::Course, &turn("", Some(("create_course", "{\"words\":[]}")))).0 < 0.5);

        assert_eq!(score_rule(RuleTask::Danish, "Det er en god spørgsmål, og du skal bruge pan.").0, 1.0);
        assert_eq!(score_rule(RuleTask::Danish, "The difference is that you use pan.").0, 0.0);
        assert_eq!(score_rule(RuleTask::ShortWithPlayer, "You say [[say: dziękuję]].").0, 1.0);
        assert_eq!(score_rule(RuleTask::PartnerStaysPolish, "Przepraszam, nie rozumiem.").0, 1.0);
        assert_eq!(score_rule(RuleTask::PartnerStaysPolish, "Oh nice, I live here too!").0, 0.0);
        assert!(items().iter().filter(|i| i.part == "polish").count() >= 15);
    }
}

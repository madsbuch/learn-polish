//! The AI cost center: every paid request is logged with the feature that
//! made it, so Settings can show what the app costs per day and per feature.
//!
//! OpenRouter reports the price of each chat completion (`usage.cost`, in
//! USD credits). Its speech endpoint reports no price, so speech is logged
//! with a NULL cost and shown as a count. kie.ai reports the Suno credits a
//! task used; those are converted at [`KIE_USD_PER_CREDIT`].

use diesel::dsl::sum;
use diesel::prelude::*;
use serde::Serialize;

use crate::db::{self, Conn};
use crate::schema::ai_costs;
use crate::error::Result;

/// kie.ai sells credits at roughly $0.005 each.
pub const KIE_USD_PER_CREDIT: f64 = 0.005;

/// Features as they are stored and shown.
pub const TUTOR: &str = "tutor";
pub const EXAMINER: &str = "examiner";
pub const SONGWRITER: &str = "songwriter";
pub const SONGS: &str = "songs";
pub const EXPLAIN: &str = "explain";
pub const SPEECH: &str = "speech";
pub const KEY_TEST: &str = "key test";

/// Logs one request. A failure to log must never fail the feature, so callers
/// ignore the result; it is returned for tests.
pub fn record(
    conn: &mut Conn,
    feature: &str,
    provider: &str,
    model: &str,
    cost_usd: Option<f64>,
    detail: &str,
) -> Result<()> {
    record_once(conn, feature, provider, model, cost_usd, detail, None)
}

/// [`record`], but a second entry with the same `reference` is ignored.
pub fn record_once(
    conn: &mut Conn,
    feature: &str,
    provider: &str,
    model: &str,
    cost_usd: Option<f64>,
    detail: &str,
    reference: Option<&str>,
) -> Result<()> {
    let day = db::today(conn);
    diesel::insert_or_ignore_into(ai_costs::table)
        .values((
            ai_costs::day.eq(day),
            ai_costs::feature.eq(feature),
            ai_costs::provider.eq(provider),
            ai_costs::model.eq(model),
            ai_costs::cost_usd.eq(cost_usd.filter(|c| c.is_finite() && *c >= 0.0)),
            ai_costs::detail.eq(detail),
            ai_costs::reference.eq(reference),
            ai_costs::created_at.eq(db::now_secs()),
        ))
        .execute(conn)?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FeatureCost {
    pub feature: String,
    pub cost_usd: f64,
    pub calls: i64,
    /// Requests whose price the provider did not report.
    pub unpriced: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayCost {
    pub day: i64,
    pub cost_usd: f64,
    pub calls: i64,
    pub features: Vec<FeatureCost>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CostSummary {
    pub today: i64,
    pub days: i64,
    pub today_usd: f64,
    /// Total over the window.
    pub total_usd: f64,
    pub all_time_usd: f64,
    /// Per feature over the window, most expensive first.
    pub features: Vec<FeatureCost>,
    /// Days with any activity, newest first.
    pub per_day: Vec<DayCost>,
}

/// Costs over the last `days` days (today included).
pub fn summary(conn: &mut Conn, days: i64) -> Result<CostSummary> {
    let days = days.clamp(1, 3650);
    let today = db::today(conn);
    let from = today - days + 1;

    let rows: Vec<(i64, String, Option<f64>)> = ai_costs::table
        .filter(ai_costs::day.ge(from))
        .select((ai_costs::day, ai_costs::feature, ai_costs::cost_usd))
        .load(conn)?;

    // Per day, per feature: the cost, how many requests, how many unpriced.
    let mut per_day: Vec<DayCost> = Vec::new();
    for (day, feature, cost) in rows {
        let entry = match per_day.iter_mut().position(|d| d.day == day) {
            Some(i) => &mut per_day[i],
            None => {
                per_day.push(DayCost { day, cost_usd: 0.0, calls: 0, features: Vec::new() });
                per_day.last_mut().expect("just pushed")
            }
        };
        entry.cost_usd += cost.unwrap_or(0.0);
        entry.calls += 1;
        let line = match entry.features.iter_mut().position(|f| f.feature == feature) {
            Some(i) => &mut entry.features[i],
            None => {
                entry.features.push(FeatureCost { feature, cost_usd: 0.0, calls: 0, unpriced: 0 });
                entry.features.last_mut().expect("just pushed")
            }
        };
        line.cost_usd += cost.unwrap_or(0.0);
        line.calls += 1;
        line.unpriced += i64::from(cost.is_none());
    }
    per_day.sort_by(|a, b| b.day.cmp(&a.day));
    let mut features: Vec<FeatureCost> = Vec::new();
    for day in &mut per_day {
        day.features.sort_by(|a, b| b.cost_usd.total_cmp(&a.cost_usd).then(a.feature.cmp(&b.feature)));
        for cost in &day.features {
            match features.iter_mut().find(|f| f.feature == cost.feature) {
                Some(total) => {
                    total.cost_usd += cost.cost_usd;
                    total.calls += cost.calls;
                    total.unpriced += cost.unpriced;
                }
                None => features.push(cost.clone()),
            }
        }
    }
    features.sort_by(|a, b| b.cost_usd.total_cmp(&a.cost_usd).then(b.calls.cmp(&a.calls)));

    let total_usd = features.iter().map(|f| f.cost_usd).sum();
    let today_usd = per_day.iter().find(|d| d.day == today).map_or(0.0, |d| d.cost_usd);
    let all_time_usd: f64 =
        ai_costs::table.select(sum(ai_costs::cost_usd)).first::<Option<f64>>(conn)?.unwrap_or(0.0);

    Ok(CostSummary { today, days, today_usd, total_usd, all_time_usd, features, per_day })
}

pub mod commands {
    use tauri::State;

    use crate::db::Db;
    use crate::error::Result;

    /// What the AI features cost, per day and per feature, over the last `days`.
    #[tauri::command]
    pub async fn costs_summary(db: State<'_, Db>, days: i64) -> Result<super::CostSummary> {
        super::summary(&mut db.lock(), days)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_per_day_and_per_feature() {
        let path = std::env::temp_dir()
            .join("learn-polish-tests")
            .join(format!("costs-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let conn = &mut db::open(&path).unwrap();
        record(conn, TUTOR, "openrouter", "m", Some(0.01), "").unwrap();
        record(conn, TUTOR, "openrouter", "m", Some(0.02), "").unwrap();
        record(conn, SPEECH, "openrouter", "tts", None, "12 characters").unwrap();
        record_once(conn, SONGS, "kie.ai", "V5", Some(0.06), "12 credits", Some("kie:t1")).unwrap();
        // Polled again after a failed download: not charged twice.
        record_once(conn, SONGS, "kie.ai", "V5", Some(0.06), "12 credits", Some("kie:t1")).unwrap();
        let today = db::today(conn);
        let earlier = |day: i64, cost: f64| {
            (
                ai_costs::day.eq(day),
                ai_costs::feature.eq(TUTOR),
                ai_costs::provider.eq("openrouter"),
                ai_costs::cost_usd.eq(Some(cost)),
                ai_costs::created_at.eq(0),
            )
        };
        // The second is outside a 7-day window, but counted all-time.
        diesel::insert_into(ai_costs::table)
            .values(&vec![earlier(today - 3, 0.5), earlier(today - 30, 1.0)])
            .execute(conn)
            .unwrap();

        let s = summary(conn, 7).unwrap();
        assert!((s.today_usd - 0.09).abs() < 1e-9);
        assert!((s.total_usd - 0.59).abs() < 1e-9);
        assert!((s.all_time_usd - 1.59).abs() < 1e-9);
        assert_eq!(s.per_day.len(), 2);
        assert_eq!(s.per_day[0].day, today, "newest first");
        assert_eq!(s.per_day[0].calls, 4);
        assert_eq!(s.features[0].feature, TUTOR);
        assert_eq!(s.features[0].calls, 3);
        let speech = s.features.iter().find(|f| f.feature == SPEECH).unwrap();
        assert_eq!((speech.calls, speech.unpriced), (1, 1));
    }
}

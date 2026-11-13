//! The AI cost center: every paid request is logged with the feature that
//! made it, so Settings can show what the app costs per day and per feature.
//!
//! OpenRouter reports the price of each chat completion (`usage.cost`, in
//! USD credits). Its speech endpoint reports no price, so speech is logged
//! with a NULL cost and shown as a count. kie.ai reports the Suno credits a
//! task used; those are converted at [`KIE_USD_PER_CREDIT`].

use rusqlite::{params, Connection};
use serde::Serialize;

use crate::db;
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
    conn: &Connection,
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
    conn: &Connection,
    feature: &str,
    provider: &str,
    model: &str,
    cost_usd: Option<f64>,
    detail: &str,
    reference: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO ai_costs (day, feature, provider, model, cost_usd, detail, ref, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            db::today(conn),
            feature,
            provider,
            model,
            cost_usd.filter(|c| c.is_finite() && *c >= 0.0),
            detail,
            reference,
            db::now_secs()
        ],
    )?;
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
pub fn summary(conn: &Connection, days: i64) -> Result<CostSummary> {
    let days = days.clamp(1, 3650);
    let today = db::today(conn);
    let from = today - days + 1;

    let mut stmt = conn.prepare(
        "SELECT day, feature, COALESCE(SUM(cost_usd), 0), COUNT(*),
                SUM(CASE WHEN cost_usd IS NULL THEN 1 ELSE 0 END)
         FROM ai_costs WHERE day >= ?1
         GROUP BY day, feature
         ORDER BY day DESC, 3 DESC, feature",
    )?;
    let rows = stmt.query_map([from], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            FeatureCost { feature: r.get(1)?, cost_usd: r.get(2)?, calls: r.get(3)?, unpriced: r.get(4)? },
        ))
    })?;

    let mut per_day: Vec<DayCost> = Vec::new();
    let mut features: Vec<FeatureCost> = Vec::new();
    for row in rows {
        let (day, cost) = row?;
        match features.iter_mut().find(|f| f.feature == cost.feature) {
            Some(total) => {
                total.cost_usd += cost.cost_usd;
                total.calls += cost.calls;
                total.unpriced += cost.unpriced;
            }
            None => features.push(cost.clone()),
        }
        if per_day.last().map(|d| d.day) != Some(day) {
            per_day.push(DayCost { day, cost_usd: 0.0, calls: 0, features: Vec::new() });
        }
        let entry = per_day.last_mut().expect("just pushed");
        entry.cost_usd += cost.cost_usd;
        entry.calls += cost.calls;
        entry.features.push(cost);
    }
    features.sort_by(|a, b| b.cost_usd.total_cmp(&a.cost_usd).then(b.calls.cmp(&a.calls)));

    let total_usd = features.iter().map(|f| f.cost_usd).sum();
    let today_usd = per_day.iter().find(|d| d.day == today).map_or(0.0, |d| d.cost_usd);
    let all_time_usd: f64 =
        conn.query_row("SELECT COALESCE(SUM(cost_usd), 0) FROM ai_costs", [], |r| r.get(0))?;

    Ok(CostSummary { today, days, today_usd, total_usd, all_time_usd, features, per_day })
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
        let conn = db::open(&path).unwrap();
        record(&conn, TUTOR, "openrouter", "m", Some(0.01), "").unwrap();
        record(&conn, TUTOR, "openrouter", "m", Some(0.02), "").unwrap();
        record(&conn, SPEECH, "openrouter", "tts", None, "12 characters").unwrap();
        record_once(&conn, SONGS, "kie.ai", "V5", Some(0.06), "12 credits", Some("kie:t1")).unwrap();
        // Polled again after a failed download: not charged twice.
        record_once(&conn, SONGS, "kie.ai", "V5", Some(0.06), "12 credits", Some("kie:t1")).unwrap();
        let today = db::today(&conn);
        conn.execute(
            "INSERT INTO ai_costs (day, feature, provider, cost_usd, created_at) VALUES (?1, 'tutor', 'openrouter', 0.5, 0)",
            [today - 3],
        )
        .unwrap();
        // Outside a 7-day window, but counted all-time.
        conn.execute(
            "INSERT INTO ai_costs (day, feature, provider, cost_usd, created_at) VALUES (?1, 'tutor', 'openrouter', 1.0, 0)",
            [today - 30],
        )
        .unwrap();

        let s = summary(&conn, 7).unwrap();
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

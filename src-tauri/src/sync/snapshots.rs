//! The computer's daily copies of the whole database (as Tally's).
//!
//! Sync mirrors: a lesson deleted by mistake on the phone is gone from the
//! computer too within minutes. These snapshots are what lets the learner go
//! back past one: one a day for the last two weeks, then one a week for about
//! three months. Each is a complete, consistent database (`VACUUM INTO`),
//! named by its day, and marked as a copy (instance.rs) like the backups
//! db.rs takes, so one put back in place takes a new device id instead of
//! posing as this device with an older history.
//!
//! They go in `snapshots/`, beside `backups/` rather than in it: the backups
//! there are kept by db.rs's own rule (the newest two of each kind), these by
//! their dates, and in separate folders neither rule can ever remove the
//! other's copies. The learner looking for "yesterday's data" also finds
//! them by name, apart from the copies taken before an upgrade.

use std::path::{Path, PathBuf};

use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use diesel::sql_types::Text;

use crate::db::Conn;
use crate::error::Result;

/// Days kept one per day. Fewer than Tally keeps: Polski's database holds
/// song audio, so each copy can be large — a week of daily copies and a few
/// weekly ones still cover noticing a sync mistake weeks later.
pub const DAILY: i64 = 7;
/// Beyond those, one per week back to this many days.
pub const WEEKLY_UNTIL: i64 = 28;
/// How often the app looks whether today's is taken yet (it's taken once a
/// day; the app may run for days, or start just before midnight).
pub const EVERY: std::time::Duration = std::time::Duration::from_secs(3600);

const PREFIX: &str = "polski-";
const SUFFIX: &str = ".sqlite";

/// The folder the snapshots of the database at `db_path` go in.
pub fn dir_for(db_path: &Path) -> Option<PathBuf> {
    db_path.parent().map(|p| p.join("snapshots"))
}

/// Days since 1970-01-01 of a "YYYY-MM-DD" date.
pub fn day_number(date: &str) -> Option<i64> {
    let mut p = date.split('-');
    let (y, m, d): (i64, i64, i64) = (p.next()?.parse().ok()?, p.next()?.parse().ok()?, p.next()?.parse().ok()?);
    if p.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Days-from-civil (Howard Hinnant), the inverse of commands::civil_date.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// A day number as "YYYY-MM-DD".
pub fn date(days: i64) -> String {
    let (y, m, d) = crate::commands::civil_date(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Today's date (UTC), "YYYY-MM-DD": one snapshot per UTC day is enough,
/// and it needs no time zone.
pub fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0);
    date(secs.div_euclid(86_400))
}

/// Which of `dates` to keep, given today: the last [`DAILY`] days, then the
/// newest of each week (Monday to Sunday) back to [`WEEKLY_UNTIL`] days.
pub fn keep(dates: &[String], today: &str) -> Vec<String> {
    let Some(t) = day_number(today) else { return dates.to_vec() };
    let mut by_week: std::collections::BTreeMap<i64, (i64, &String)> = Default::default();
    let mut out = Vec::new();
    for d in dates {
        let Some(n) = day_number(d) else { continue };
        let age = t - n;
        if (0..DAILY).contains(&age) {
            out.push(d.clone());
        } else if (DAILY..WEEKLY_UNTIL).contains(&age) {
            // 1970-01-01 was a Thursday; +3 makes weeks start on Monday.
            let week = (n + 3).div_euclid(7);
            let e = by_week.entry(week).or_insert((n, d));
            if n > e.0 {
                *e = (n, d);
            }
        }
    }
    out.extend(by_week.into_values().map(|(_, d)| d.clone()));
    out.sort();
    out
}

fn date_of(name: &str) -> Option<String> {
    let d = name.strip_prefix(PREFIX)?.strip_suffix(SUFFIX)?;
    day_number(d).map(|_| d.to_string())
}

/// Takes today's snapshot of the database at `db_path` into `dir` if there
/// isn't one yet, then removes the ones no longer kept. Returns the path
/// written, if any.
///
/// The copy is read through a connection of its own, not the app's: a large
/// database takes a while to copy, and the screens shouldn't wait for it. A
/// read sees one consistent moment of the database (WAL) while the app
/// carries on writing.
pub fn run(db_path: &Path, dir: &Path, today: &str) -> Result<Option<PathBuf>> {
    std::fs::create_dir_all(dir)?;
    let target = dir.join(format!("{PREFIX}{today}{SUFFIX}"));
    let mut written = None;
    if !target.exists() {
        // Written under another name and renamed when whole: a copy cut off
        // half-way (the computer slept, the app quit) is never taken for
        // today's, and the next try starts over.
        let partial = dir.join(format!("{PREFIX}{today}.partial"));
        let _ = std::fs::remove_file(&partial);
        let mut conn = Conn::establish(&db_path.to_string_lossy())?;
        conn.batch_execute("PRAGMA busy_timeout = 15000;")?;
        diesel::sql_query("VACUUM INTO ?").bind::<Text, _>(partial.to_string_lossy().to_string()).execute(&mut conn)?;
        drop(conn);
        super::instance::mark_copy(&partial)?;
        std::fs::rename(&partial, &target)?;
        written = Some(target);
    }
    let dates: Vec<String> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok()?.file_name().to_str().and_then(date_of))
        .collect();
    let kept = keep(&dates, today);
    for d in dates.iter().filter(|d| !kept.contains(d)) {
        let _ = std::fs::remove_file(dir.join(format!("{PREFIX}{d}{SUFFIX}")));
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::sync::instance;

    #[test]
    fn day_numbers_are_right() {
        assert_eq!(day_number("1970-01-01"), Some(0));
        assert_eq!(day_number("2000-03-01"), Some(11_017));
        assert_eq!(day_number("2026-11-16"), Some(20_773));
        assert_eq!(day_number("2026-13-01"), None);
        assert_eq!(day_number("nope"), None);
        for n in [0, 59, 60, 11_016, 20_773, 30_000] {
            assert_eq!(day_number(&date(n)), Some(n));
        }
        assert_eq!(day_number(&today()).map(date), Some(today()));
    }

    #[test]
    fn a_year_of_daily_snapshots_thins_out_as_promised() {
        let today = day_number("2026-11-16").unwrap();
        let all: Vec<String> = (0..365).map(|age| date(today - age)).collect();
        let kept = keep(&all, &date(today));
        // Every one of the last DAILY days…
        for age in 0..DAILY {
            assert!(kept.contains(&date(today - age)), "day -{age}");
        }
        // …then one per week (Monday to Sunday), the newest of it within the
        // weekly range, to WEEKLY_UNTIL days.
        let older: Vec<i64> = kept.iter().map(|d| day_number(d).unwrap()).filter(|n| today - n >= DAILY).collect();
        let weeks: std::collections::BTreeSet<i64> = older.iter().map(|n| (n + 3).div_euclid(7)).collect();
        assert_eq!(weeks.len(), older.len(), "one per week: {older:?}");
        let weeks_in_range = (WEEKLY_UNTIL - DAILY) / 7;
        let count = i64::try_from(older.len()).unwrap();
        assert!((weeks_in_range - 1..=weeks_in_range + 1).contains(&count), "{older:?}");
        for n in &older {
            let week_end = (n + 3).div_euclid(7) * 7 - 3 + 6;
            assert!(*n == week_end || today - week_end < DAILY, "{} isn't its week's newest", date(*n));
        }
        // …and nothing older.
        assert!(older.iter().all(|n| today - n < WEEKLY_UNTIL));
    }

    #[test]
    fn gaps_keep_what_there_is() {
        // Computer off for a while: the newest one of each week is kept anyway.
        let kept = keep(&["2026-10-22".into(), "2026-10-23".into(), "2026-11-16".into()], "2026-11-16");
        assert_eq!(kept, vec!["2026-10-23".to_string(), "2026-11-16".to_string()]);
    }

    #[test]
    fn takes_one_a_day_and_prunes() {
        let root = std::env::temp_dir().join("learn-polish-tests").join(format!("snapshots-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("learn-polish.db");
        let mut conn = db::open(&path).unwrap();
        conn.batch_execute(&format!(
            "INSERT INTO memories (id, content, created_at, updated_at) VALUES ('{}', 'Oats for breakfast', 1, 1)",
            crate::ids::MemoryId::new()
        ))
        .unwrap();
        let dir = dir_for(&path).unwrap();
        assert_eq!(dir, root.join("snapshots"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("polski-2025-01-01.sqlite"), b"old").unwrap();
        std::fs::write(dir.join("notes.txt"), b"not ours").unwrap();

        // Taken while the app holds the database open, as it does.
        let written = run(&path, &dir, "2026-11-16").unwrap().unwrap();
        assert!(run(&path, &dir, "2026-11-16").unwrap().is_none(), "one a day");
        assert!(!dir.join("polski-2025-01-01.sqlite").exists());
        assert!(dir.join("notes.txt").exists());
        assert!(!dir.join("polski-2026-11-16.partial").exists());

        // The snapshot is a whole database…
        let mut snap = Conn::establish(&written.to_string_lossy()).unwrap();
        #[derive(QueryableByName)]
        struct Content {
            #[diesel(sql_type = Text)]
            content: String,
        }
        let c = diesel::sql_query("SELECT content FROM memories").get_result::<Content>(&mut snap).unwrap();
        assert_eq!(c.content, "Oats for breakfast");
        // …marked as a copy: put back in place, it's noticed and takes a new id.
        let marker = db::get_setting(&mut snap, instance::MARKER).unwrap().unwrap();
        assert!(marker.starts_with("copy:"), "{marker}");
        drop(snap);
        let id = crate::sync::device_id(&mut conn).unwrap();
        drop(conn);
        std::fs::copy(&written, &path).unwrap();
        let _ = std::fs::remove_file(root.join("learn-polish.db-wal"));
        let _ = std::fs::remove_file(root.join("learn-polish.db-shm"));
        let mut restored = db::open(&path).unwrap();
        assert_ne!(crate::sync::device_id(&mut restored).unwrap(), id);
    }
}

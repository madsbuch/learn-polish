//! An SM-2 style scheduler, trimmed to the four grades the UI can produce.

use rusqlite::{Connection, OptionalExtension};

use crate::error::Result;

pub const AGAIN: i64 = 0;
pub const HARD: i64 = 1;
pub const GOOD: i64 = 2;
pub const EASY: i64 = 3;

const MIN_EASE: f64 = 1.3;
const MAX_EASE: f64 = 2.8;
const START_EASE: f64 = 2.5;
/// Beyond this the interval stops being useful for a daily-practice app.
const MAX_INTERVAL: i64 = 365;

#[derive(Debug, Clone, Copy)]
pub struct ReviewState {
    pub due_day: i64,
    pub interval: i64,
    pub ease: f64,
    pub reps: i64,
    pub lapses: i64,
    pub last_day: i64,
}

impl ReviewState {
    pub fn fresh(today: i64) -> Self {
        ReviewState {
            due_day: today,
            interval: 0,
            ease: START_EASE,
            reps: 0,
            lapses: 0,
            last_day: today,
        }
    }
}

/// Applies a grade and returns the new state. Pure, so it is easy to test.
pub fn grade(state: ReviewState, grade: i64, today: i64) -> ReviewState {
    let mut next = state;
    next.last_day = today;

    if grade <= AGAIN {
        next.lapses += 1;
        next.reps = 0;
        next.interval = 0;
        next.ease = (next.ease - 0.20).max(MIN_EASE);
        // Due again the same day, so a missed item comes back before the
        // session ends rather than tomorrow.
        next.due_day = today;
        return next;
    }

    next.ease = match grade {
        HARD => (next.ease - 0.15).max(MIN_EASE),
        EASY => (next.ease + 0.10).min(MAX_EASE),
        _ => next.ease,
    };

    next.interval = match next.reps {
        0 => match grade {
            HARD => 1,
            EASY => 3,
            _ => 1,
        },
        1 => match grade {
            HARD => 2,
            EASY => 5,
            _ => 3,
        },
        _ => {
            let factor = match grade {
                HARD => 1.2,
                EASY => next.ease * 1.3,
                _ => next.ease,
            };
            ((next.interval.max(1) as f64) * factor).round() as i64
        }
    }
    .clamp(1, MAX_INTERVAL);

    next.reps += 1;
    next.due_day = today + next.interval;
    next
}

pub fn load(conn: &Connection, kind: &str, item_id: i64) -> Result<Option<ReviewState>> {
    Ok(conn
        .query_row(
            "SELECT due_day, interval, ease, reps, lapses, last_day
             FROM reviews WHERE kind = ?1 AND item_id = ?2",
            rusqlite::params![kind, item_id],
            |r| {
                Ok(ReviewState {
                    due_day: r.get(0)?,
                    interval: r.get(1)?,
                    ease: r.get(2)?,
                    reps: r.get(3)?,
                    lapses: r.get(4)?,
                    last_day: r.get(5)?,
                })
            },
        )
        .optional()?)
}

pub fn save(conn: &Connection, kind: &str, item_id: i64, state: &ReviewState) -> Result<()> {
    conn.execute(
        "INSERT INTO reviews (kind, item_id, due_day, interval, ease, reps, lapses, last_day)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(kind, item_id) DO UPDATE SET
             due_day = excluded.due_day, interval = excluded.interval,
             ease = excluded.ease, reps = excluded.reps,
             lapses = excluded.lapses, last_day = excluded.last_day",
        rusqlite::params![
            kind,
            item_id,
            state.due_day,
            state.interval,
            state.ease,
            state.reps,
            state.lapses,
            state.last_day
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn good_answers_grow_the_interval() {
        let mut state = ReviewState::fresh(0);
        state = grade(state, GOOD, 0);
        assert_eq!(state.interval, 1);
        state = grade(state, GOOD, 1);
        assert_eq!(state.interval, 3);
        let third = grade(state, GOOD, 4);
        assert!(third.interval > 3, "got {}", third.interval);
        assert_eq!(third.due_day, 4 + third.interval);
    }

    #[test]
    fn a_lapse_resets_and_lowers_ease() {
        let mut state = ReviewState::fresh(0);
        state = grade(state, GOOD, 0);
        state = grade(state, GOOD, 1);
        let lapsed = grade(state, AGAIN, 4);
        assert_eq!(lapsed.reps, 0);
        assert_eq!(lapsed.interval, 0);
        assert_eq!(lapsed.due_day, 4);
        assert_eq!(lapsed.lapses, 1);
        assert!(lapsed.ease < state.ease);
    }

    #[test]
    fn ease_stays_within_bounds() {
        let mut state = ReviewState::fresh(0);
        for day in 0..40 {
            state = grade(state, HARD, day);
        }
        assert!(state.ease >= MIN_EASE);

        let mut state = ReviewState::fresh(0);
        for day in 0..40 {
            state = grade(state, EASY, day);
        }
        assert!(state.ease <= MAX_EASE);
        assert!(state.interval <= MAX_INTERVAL);
    }
}

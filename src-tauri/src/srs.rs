//! An SM-2 style scheduler, trimmed to the four grades the UI can produce.

use diesel::prelude::*;

#[allow(unused_imports)]
use crate::ids::*;
use crate::db::Conn;
use crate::schema::reviews;

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

/// One item's review state: a `reviews` row without its key.
#[derive(Debug, Clone, Copy, Queryable, Selectable, Insertable, AsChangeset)]
#[diesel(table_name = reviews)]
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
///
/// Spacing is the point: an item counts once a day. A right answer on the
/// same day as its last review (`last_day`, once it has been reviewed at
/// all) changes nothing — five right answers in one afternoon do not make a
/// word "stick" for three weeks. A miss still lapses it, whenever it comes.
///
/// HARD is a miss that was nearly there (a slip of a letter, one word off,
/// a sentence misheard): it comes back sooner, but is not started over. It
/// keeps the item's reps and never lengthens its gap — on another day it
/// halves it (at least a day); on the same day, like GOOD, it changes
/// nothing.
pub fn grade(state: ReviewState, grade: i64, today: i64) -> ReviewState {
    let mut next = state;

    if grade <= AGAIN {
        next.last_day = today;
        next.lapses += 1;
        next.reps = 0;
        next.interval = 0;
        next.ease = (next.ease - 0.20).max(MIN_EASE);
        // Due again at once: it is the first to come back.
        next.due_day = today;
        return next;
    }

    // Reviewed already today (a new item, never reviewed, has no reps and
    // no lapses): counted once a day.
    let reviewed = state.reps > 0 || state.lapses > 0;
    if reviewed && state.last_day >= today {
        return next;
    }
    next.last_day = today;

    if grade == HARD {
        next.ease = (next.ease - 0.15).max(MIN_EASE);
        next.interval = (next.interval / 2).max(1);
        next.due_day = today + next.interval;
        return next;
    }

    next.ease = match grade {
        EASY => (next.ease + 0.10).min(MAX_EASE),
        _ => next.ease,
    };

    next.interval = match next.reps {
        0 => match grade {
            EASY => 3,
            _ => 1,
        },
        1 => match grade {
            EASY => 5,
            _ => 3,
        },
        _ => {
            let factor = match grade {
                EASY => next.ease * 1.3,
                _ => next.ease,
            };
            crate::convert::round(crate::convert::real(next.interval.max(1)) * factor)
        }
    }
    .clamp(1, MAX_INTERVAL);

    next.reps += 1;
    next.due_day = today + next.interval;
    next
}

pub fn load(conn: &mut Conn, kind: &str, item_id: &str) -> Result<Option<ReviewState>> {
    Ok(reviews::table
        .find((kind, item_id))
        .select(ReviewState::as_select())
        .first(conn)
        .optional()?)
}

pub fn save(conn: &mut Conn, kind: &str, item_id: &str, state: &ReviewState) -> Result<()> {
    diesel::insert_into(reviews::table)
        .values((reviews::kind.eq(kind), reviews::item_id.eq(item_id), state))
        .on_conflict((reviews::kind, reviews::item_id))
        .do_update()
        .set(state)
        .execute(conn)?;
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
    fn right_answers_count_once_a_day() {
        let mut state = ReviewState::fresh(10);
        for _ in 0..5 {
            state = grade(state, GOOD, 10);
        }
        assert_eq!((state.reps, state.interval, state.due_day), (1, 1, 11), "five in a day are one review");
        // The next day it grows; a miss the same day still lapses.
        state = grade(state, GOOD, 11);
        assert_eq!((state.reps, state.interval), (2, 3));
        let lapsed = grade(state, AGAIN, 11);
        assert_eq!((lapsed.reps, lapsed.lapses), (0, 1));
        // Right again after the lapse, that day: no credit until tomorrow.
        assert_eq!(grade(lapsed, GOOD, 11).reps, 0);
        assert_eq!(grade(lapsed, GOOD, 12).reps, 1);
    }

    #[test]
    fn a_near_miss_never_lengthens_the_gap() {
        let mut state = ReviewState::fresh(0);
        for day in [0, 1, 4, 12] {
            state = grade(state, GOOD, day);
        }
        assert_eq!(state.interval, 20);
        for (day, label) in [(12, "same day"), (40, "a later day")] {
            let hard = grade(state, HARD, day);
            let good = grade(state, GOOD, day);
            assert!(hard.interval <= good.interval, "{label}: HARD {} against GOOD {}", hard.interval, good.interval);
            assert!(hard.interval <= state.interval, "{label}: a miss does not grow it");
            assert_eq!(hard.reps, state.reps, "{label}: nor start it over");
        }
        assert_eq!(grade(state, HARD, 40).interval, 10, "halved, on another day");
        let new = grade(ReviewState::fresh(3), HARD, 3);
        assert!(new.interval <= grade(ReviewState::fresh(3), GOOD, 3).interval);
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

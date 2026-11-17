//! Number conversions without `as` (Clippy denies `as_conversions`): each
//! says what happens at the edges instead of truncating or wrapping
//! silently.

/// A length or count, as the i64 the database and the screens use
/// (saturating: no list is that long).
pub fn count(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// An i64 used as a length, index or capacity: negatives are 0.
pub fn index(n: i64) -> usize {
    usize::try_from(n).unwrap_or(0)
}

/// An integer as a float, for ratios and scores. Exact up to 2^53 — far
/// beyond any count or day number here. This is the one place a float
/// conversion is written out.
#[allow(clippy::as_conversions)]
pub fn real(n: i64) -> f64 {
    n as f64
}

/// A length as a float, for ratios.
pub fn real_len(n: usize) -> f64 {
    real(count(n))
}

/// A float rounded to the nearest integer, saturating at the ends (NaN is 0).
#[allow(clippy::as_conversions)]
pub fn round(x: f64) -> i64 {
    x.round() as i64
}

/// A float rounded to the nearest non-negative integer, as a length.
pub fn round_len(x: f64) -> usize {
    index(round(x))
}

/// A length in bytes, as u64.
pub fn bytes(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// A u64 offset or length, as usize (saturating).
pub fn offset(n: u64) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

/// A stored i64 size or offset, as u64: negatives are 0.
pub fn unsigned(n: i64) -> u64 {
    u64::try_from(n).unwrap_or(0)
}

/// A u64 as i64 (saturating).
pub fn signed(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_are_spelled_out() {
        assert_eq!(count(3), 3);
        assert_eq!(index(-4), 0);
        assert_eq!(index(7), 7);
        assert_eq!(real(5), 5.0);
        assert_eq!(round(2.5), 3);
        assert_eq!(round(f64::NAN), 0);
        assert_eq!(round(1e30), i64::MAX);
        assert_eq!(round_len(-3.0), 0);
    }
}

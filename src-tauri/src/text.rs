//! Small text helpers shared across the app.

/// Trims and collapses inner whitespace, so "  czerwony   kolor " is stored
/// (and compared) the way it would be typed.
pub fn clean(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    #[test]
    fn whitespace_is_collapsed() {
        assert_eq!(super::clean("  jasno   niebieski \n"), "jasno niebieski");
    }
}

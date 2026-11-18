pub fn is_header_only(text: &str) -> bool {
    text.lines().filter(|line| !line.trim().is_empty()).count() <= 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_without_rows_is_empty() {
        assert!(is_header_only("Designator,Mid X\n"));
        assert!(!is_header_only("Designator\nR1\n"));
    }
}

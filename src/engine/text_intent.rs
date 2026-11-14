//! Shared deterministic text-boundary rules for agent/tool intents.
//! Callers lowercase the input once. Non-ASCII action stems intentionally
//! match inflected forms; ASCII verbs match whole tokens, never substrings.

/// A matching term is already lowercase. ASCII terms use [A-Za-z0-9_]
/// boundaries; Korean action stems use substring matching for inflection.
pub(crate) fn has_term(query: &str, term: &str) -> bool {
    if !term.is_ascii() {
        return query.contains(term);
    }
    query.match_indices(term).any(|(offset, _)| {
        let before = query[..offset].chars().last();
        let after = query[offset + term.len()..].chars().next();
        let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
        !before.is_some_and(word) && !after.is_some_and(word)
    })
}

pub(crate) fn has_any(query: &str, terms: &[&str]) -> bool {
    terms.iter().any(|term| has_term(query, term))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_word_english_and_inflected_korean_are_distinct() {
        for (text, token) in [
            ("latest", "test"),
            ("profile", "file"),
            ("thread", "read"),
            ("brunch", "run"),
            ("capitalize", "api"),
            ("test_case", "test"),
        ] {
            assert!(!has_term(text, token), "{text} is not a {token} action");
        }
        for (text, token) in [
            ("run tests", "run"),
            ("read a file", "read"),
            ("test, please", "test"),
            ("(api)", "api"),
            ("이 파일을 수정해주세요", "수정"),
            ("메모리에 저장해주세요", "저장"),
        ] {
            assert!(has_term(text, token), "{text} should match {token}");
        }
    }
}

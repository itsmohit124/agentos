//! Deterministic text normalisation shared by trigger validation and the router.

/// Splits `text` into lowercase, lightly stemmed word tokens.
///
/// Tokens are maximal runs of alphanumeric characters, so `"test-driven"` becomes
/// `["test", "driven"]`. Stemming is a small fixed set of English suffix rules (see [`stem`]); it
/// is applied identically to tasks and triggers, so it only has to be consistent, not linguistic.
pub fn normalize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| stem(&t.to_lowercase()))
        .collect()
}

/// Light suffix stemmer for lowercase tokens of at least four characters.
///
/// Applies the first matching rule of `ies → y`, `ing`, `ed`, `es`, `s` (not after `s`, `u`, `i`)
/// that leaves at least three characters, then drops a trailing `e` if three characters remain.
/// So `modules`/`module` → `modul`, `testing`/`tests` → `test`, `improving`/`improve` → `improv`.
pub fn stem(token: &str) -> String {
    if token.chars().count() < 4 || !token.is_ascii() {
        return token.to_owned();
    }
    let mut word = token.to_owned();
    let keeps = |w: &str, suffix: &str| w.len() >= suffix.len() + 3 && w.ends_with(suffix);
    if keeps(&word, "ies") {
        word.truncate(word.len() - 3);
        word.push('y');
    } else if keeps(&word, "ing") {
        word.truncate(word.len() - 3);
    } else if keeps(&word, "ed") || keeps(&word, "es") {
        word.truncate(word.len() - 2);
    } else if keeps(&word, "s") && !["ss", "us", "is"].iter().any(|s| word.ends_with(s)) {
        word.truncate(word.len() - 1);
    }
    if keeps(&word, "e") {
        word.truncate(word.len() - 1);
    }
    word
}

/// Whether `needle` occurs as a contiguous run inside `haystack`.
pub(crate) fn contains_run(haystack: &[String], needle: &[String]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_splits_lowercases_and_stems() {
        assert_eq!(
            normalize("Write TESTS first for the test-driven Parser!"),
            [
                "writ", "test", "first", "for", "the", "test", "driven", "parser"
            ]
        );
        assert_eq!(normalize("  "), Vec::<String>::new());
        assert_eq!(normalize("SKILL.md v0.1"), ["skill", "md", "v0", "1"]);
    }

    #[test]
    fn stem_maps_inflections_together() {
        let same = |a: &str, b: &str| assert_eq!(stem(a), stem(b), "{a} vs {b}");
        same("modules", "module");
        same("testing", "tests");
        same("tested", "test");
        same("improving", "improve");
        same("improved", "improves");
        same("dependencies", "dependency");
        same("reviews", "reviewing");
        same("changes", "change");
        same("coupling", "couple");
        same("processes", "process");
    }

    #[test]
    fn stem_leaves_short_and_protected_words() {
        for w in [
            "tdd", "api", "pr", "use", "status", "analysis", "class", "process",
        ] {
            assert_eq!(stem(w), w);
        }
        assert_eq!(stem("naïve"), "naïve");
    }

    #[test]
    fn contains_run_is_contiguous() {
        let hay = normalize("write the tests first");
        assert!(contains_run(&hay, &normalize("tests first")));
        assert!(!contains_run(&hay, &normalize("write tests")));
        assert!(!contains_run(&hay, &[]));
    }
}

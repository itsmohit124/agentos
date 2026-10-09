//! Task tokens and deterministic intent words.
//!
//! Matching is whole-token and ASCII. A trailing `s` is also tried so `payments` meets
//! `payment`. No model is involved.

use std::collections::BTreeSet;

/// Intent kind and the rule name.
pub fn intent_kind(task: &str) -> (String, String) {
    let words = raw_words(task);
    if let Some(word) = first_of(&words, &["why", "what", "how", "where", "which", "explain"]) {
        return ("question".to_owned(), format!("question-word:{word}"));
    }
    if task.trim_end().ends_with('?') {
        return ("question".to_owned(), "question-mark".to_owned());
    }
    if let Some(word) = first_of(
        &words,
        &[
            "bug",
            "fix",
            "fixed",
            "fixing",
            "failing",
            "broken",
            "crash",
            "regression",
        ],
    ) {
        return ("bugfix".to_owned(), format!("bugfix-word:{word}"));
    }
    if let Some(word) = first_of(
        &words,
        &["refactor", "rename", "extract", "cleanup", "restructure"],
    ) {
        return ("refactor".to_owned(), format!("refactor-word:{word}"));
    }
    if let Some(word) = first_of(&words, &["review", "audit"]) {
        return ("review".to_owned(), format!("review-word:{word}"));
    }
    ("feature".to_owned(), "default-feature".to_owned())
}

/// True when the task states a mandatory constraint.
pub fn is_mandatory(task: &str) -> bool {
    raw_words(task)
        .iter()
        .any(|word| matches!(word.as_str(), "must" | "always" | "required" | "constraint"))
}

/// Tokens used to route bounded contexts. Intent words and function words are removed.
pub fn task_tokens(task: &str) -> BTreeSet<String> {
    tokens(task)
}

/// Tokens passed to FTS. Same as [`task_tokens`].
pub fn query_tokens(task: &str) -> Vec<String> {
    task_tokens(task).into_iter().collect()
}

/// Name, summary, and identifier tokens.
pub fn tokens(value: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut current = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            if ch.is_ascii_uppercase()
                && current
                    .chars()
                    .last()
                    .is_some_and(|prev| prev.is_ascii_lowercase())
            {
                push_token(&mut out, &current);
                current.clear();
            }
            current.push(ch.to_ascii_lowercase());
        } else {
            push_token(&mut out, &current);
            current.clear();
        }
    }
    push_token(&mut out, &current);
    out
}

fn raw_words(task: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    for ch in task.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            if current.len() >= 3 {
                words.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
        }
    }
    if current.len() >= 3 {
        words.push(current);
    }
    words
}

fn first_of(words: &[String], needles: &[&str]) -> Option<String> {
    for needle in needles {
        if words.iter().any(|word| word == needle) {
            return Some((*needle).to_owned());
        }
    }
    None
}

fn push_token(out: &mut BTreeSet<String>, token: &str) {
    if token.len() < 3 || token.chars().all(|ch| ch.is_ascii_digit()) || is_stopword(token) {
        return;
    }
    out.insert(token.to_owned());
    if let Some(stem) = token.strip_suffix('s')
        && stem.len() >= 3
        && !is_stopword(stem)
    {
        out.insert(stem.to_owned());
    }
}

fn is_stopword(token: &str) -> bool {
    matches!(
        token,
        "the"
            | "and"
            | "for"
            | "are"
            | "was"
            | "were"
            | "been"
            | "being"
            | "this"
            | "that"
            | "its"
            | "with"
            | "from"
            | "into"
            | "via"
            | "when"
            | "also"
            | "must"
            | "always"
            | "required"
            | "every"
            | "does"
            | "did"
            | "have"
            | "has"
            | "had"
            | "not"
            | "yes"
            | "then"
            | "than"
            | "but"
            | "can"
            | "could"
            | "should"
            | "would"
            | "will"
            | "just"
            | "only"
            | "about"
            | "over"
            | "under"
            | "after"
            | "before"
            | "during"
            | "through"
            | "using"
            | "use"
            | "used"
            | "new"
            | "all"
            | "any"
            | "each"
            | "per"
            | "add"
            | "adding"
            | "update"
            | "updating"
            | "fix"
            | "fixing"
            | "fixed"
            | "failing"
            | "refactor"
            | "review"
            | "explain"
            | "implement"
            | "implementing"
            | "support"
            | "supporting"
            | "retry"
            | "retries"
            | "make"
            | "making"
            | "change"
            | "changing"
            | "test"
            | "tests"
            | "testing"
            | "file"
            | "files"
            | "code"
            | "data"
            | "app"
            | "application"
            | "layer"
            | "endpoint"
            | "endpoints"
            | "field"
            | "fields"
            | "value"
            | "name"
            | "type"
            | "path"
            | "error"
            | "errors"
            | "bug"
            | "broken"
            | "crash"
            | "method"
            | "function"
            | "class"
            | "module"
            | "how"
            | "why"
            | "what"
            | "where"
            | "which"
            | "who"
            | "your"
            | "our"
            | "their"
            | "them"
            | "they"
            | "you"
            | "such"
            | "too"
            | "very"
            | "like"
            | "out"
            | "off"
            | "down"
            | "nor"
            | "his"
            | "her"
            | "she"
            | "him"
            | "one"
            | "two"
            | "three"
            | "onto"
            | "upon"
            | "without"
            | "within"
            | "across"
            | "because"
            | "while"
            | "again"
            | "once"
            | "here"
            | "there"
            | "these"
            | "those"
            | "other"
            | "another"
            | "same"
            | "own"
    )
}

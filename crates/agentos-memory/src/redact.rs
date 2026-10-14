//! Secret-like redaction applied before a memory record is stored.
//!
//! This is a fixed list of patterns. It is not a general secret scanner.

/// Patterns replaced with `[REDACTED]` before persistence.
pub const PATTERNS: &[&str] = &[
    "PEM block from BEGIN to END when the label contains PRIVATE KEY",
    "AWS access key id: AKIA plus 16 uppercase letters or digits",
    "GitHub personal access token: ghp_ plus 36 letters or digits",
    "GitHub fine-grained token: github_pat_ plus 20 or more letters, digits, or underscores",
    "Slack token: xoxb-, xoxa-, xoxp-, xoxr-, or xoxs- plus 10 or more token characters",
    "OpenAI-style key: sk- plus 20 or more letters or digits",
    "Bearer token: the word bearer, whitespace, then 8 or more token characters",
    "Assignment: api_key, apikey, api-key, access_key, access-key, secret, token, password, passwd, or pwd, then : or =, then a non-space value",
];

const MARK: &str = "[REDACTED]";

/// Replaces supported secret-like patterns. Other text is unchanged.
pub fn redact(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut index = 0;
    while index < bytes.len() {
        if let Some(len) = secret_len(input, index) {
            out.push_str(MARK);
            index += len;
            continue;
        }
        let ch = input[index..]
            .chars()
            .next()
            .expect("index is on a char boundary");
        out.push(ch);
        index += ch.len_utf8();
    }
    out
}

fn secret_len(input: &str, index: usize) -> Option<usize> {
    pem_len(input, index)
        .or_else(|| prefixed(input, index, "github_pat_", is_github_pat_char, 20))
        .or_else(|| prefixed(input, index, "ghp_", is_ascii_alnum, 36))
        .or_else(|| slack_len(input, index))
        .or_else(|| aws_len(input, index))
        .or_else(|| prefixed(input, index, "sk-", is_ascii_alnum, 20))
        .or_else(|| bearer_len(input, index))
        .or_else(|| assignment_len(input, index))
}

fn pem_len(input: &str, index: usize) -> Option<usize> {
    const BEGIN: &str = "-----BEGIN ";
    let rest = input.get(index..)?;
    if !rest.starts_with(BEGIN) {
        return None;
    }
    let line_end = rest.find('\n').unwrap_or(rest.len());
    let header = &rest[..line_end];
    if !header.contains("PRIVATE KEY") {
        return None;
    }
    let end = rest.find("-----END ")?;
    let after = end + "-----END ".len();
    let tail = rest.get(after..)?;
    let close = tail.find("-----")?;
    Some(after + close + "-----".len())
}

fn aws_len(input: &str, index: usize) -> Option<usize> {
    let rest = input.get(index..)?;
    if !rest.starts_with("AKIA") {
        return None;
    }
    let body: String = rest.chars().skip(4).take(16).collect();
    if body.len() == 16
        && body
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit())
    {
        Some(20)
    } else {
        None
    }
}

fn slack_len(input: &str, index: usize) -> Option<usize> {
    let rest = input.get(index..)?;
    let prefixes = ["xoxb-", "xoxa-", "xoxp-", "xoxr-", "xoxs-"];
    let prefix = prefixes.iter().find(|prefix| rest.starts_with(*prefix))?;
    let body = &rest[prefix.len()..];
    let len = body
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
        .count();
    (len >= 10).then_some(prefix.len() + len)
}

fn bearer_len(input: &str, index: usize) -> Option<usize> {
    let rest = input.get(index..)?;
    let head: String = rest.chars().take(6).collect();
    if !head.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let after = rest.get(6..)?;
    if !after.starts_with(|ch: char| ch.is_whitespace()) {
        return None;
    }
    let space = after.chars().take_while(|ch| ch.is_whitespace()).count();
    let token = after.get(space..)?;
    let len = token
        .chars()
        .take_while(|ch| {
            ch.is_ascii_alphanumeric() || matches!(ch, '-' | '.' | '_' | '~' | '+' | '/' | '=')
        })
        .count();
    (len >= 8).then_some(6 + space + len)
}

fn assignment_len(input: &str, index: usize) -> Option<usize> {
    if index > 0 {
        let prev = input[..index].chars().next_back()?;
        if prev.is_ascii_alphanumeric() || prev == '_' {
            return None;
        }
    }
    let rest = input.get(index..)?;
    let keywords = [
        "access_key",
        "access-key",
        "api_key",
        "api-key",
        "apikey",
        "password",
        "passwd",
        "secret",
        "token",
        "pwd",
    ];
    let keyword = keywords.iter().find(|keyword| {
        let Some(tail) = rest.get(..keyword.len()) else {
            return false;
        };
        if !tail.eq_ignore_ascii_case(keyword) {
            return false;
        }
        !matches!(
            rest[keyword.len()..].chars().next(),
            Some(ch) if ch.is_ascii_alphanumeric() || ch == '_'
        )
    })?;
    let mut cursor = keyword.len();
    let bytes = rest.as_bytes();
    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    if cursor >= bytes.len() || (bytes[cursor] != b':' && bytes[cursor] != b'=') {
        return None;
    }
    cursor += 1;
    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    let start = cursor;
    while cursor < bytes.len() && !bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    (cursor > start).then_some(cursor)
}

fn prefixed(
    input: &str,
    index: usize,
    prefix: &str,
    body: fn(char) -> bool,
    min: usize,
) -> Option<usize> {
    let rest = input.get(index..)?;
    if !rest.starts_with(prefix) {
        return None;
    }
    let len = rest[prefix.len()..]
        .chars()
        .take_while(|ch| body(*ch))
        .count();
    (len >= min).then_some(prefix.len() + len)
}

fn is_ascii_alnum(ch: char) -> bool {
    ch.is_ascii_alphanumeric()
}

fn is_github_pat_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_supported_patterns_and_leaves_ordinary_words() {
        let raw = "\
note password=hunter2 end
key AKIAIOSFODNN7EXAMPLE
ghp_abcdefghijklmnopqrstuvwxyz0123456789
Bearer abcdefghij
sk-abcdefghijklmnopqrst
token budget
";
        let clean = redact(raw);
        assert!(!clean.contains("hunter2"));
        assert!(!clean.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(!clean.contains("ghp_abcdefghijklmnopqrstuvwxyz0123456789"));
        assert!(!clean.contains("abcdefghij"));
        assert!(!clean.contains("sk-abcdefghijklmnopqrst"));
        assert!(clean.contains("token budget"));
        assert!(clean.contains(MARK));
    }

    #[test]
    fn redacts_a_private_key_block() {
        let raw = "-----BEGIN RSA PRIVATE KEY-----\nsecret\n-----END RSA PRIVATE KEY-----\n";
        let clean = redact(raw);
        assert!(!clean.contains("PRIVATE KEY"));
        assert!(!clean.contains("secret"));
        assert!(clean.contains(MARK));
    }
}

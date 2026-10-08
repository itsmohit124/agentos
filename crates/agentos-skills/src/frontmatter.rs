//! Splitting `SKILL.md` into YAML front-matter and markdown body.
//!
//! [`read_front_matter`] consumes the reader only up to the closing delimiter, so discovery never
//! reads a skill body (progressive disclosure).

use std::io::{self, BufRead};

const DELIMITER: &str = "---";

/// Why a front-matter block could not be extracted.
#[derive(Debug, thiserror::Error)]
pub enum FrontMatterError {
    /// The file does not start with a `---` line.
    #[error("SKILL.md must start with a '---' front-matter line")]
    Missing,
    /// No closing `---` line was found.
    #[error("front-matter is not terminated by a '---' line")]
    Unterminated,
    /// Reading failed (including invalid UTF-8 inside the front-matter).
    #[error("cannot read front-matter: {0}")]
    Io(#[from] io::Error),
}

fn is_delimiter(line: &str) -> bool {
    line.trim_end_matches(['\n', '\r']).trim_end() == DELIMITER
}

/// Reads the YAML front-matter from `reader`, stopping right after the closing delimiter.
pub fn read_front_matter(reader: &mut dyn BufRead) -> Result<String, FrontMatterError> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Err(FrontMatterError::Missing);
    }
    if !is_delimiter(line.trim_start_matches('\u{feff}')) {
        return Err(FrontMatterError::Missing);
    }
    let mut yaml = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Err(FrontMatterError::Unterminated);
        }
        if is_delimiter(&line) {
            return Ok(yaml);
        }
        yaml.push_str(&line);
    }
}

/// Splits a whole `SKILL.md` into `(front_matter, body)`.
pub fn split(text: &str) -> Result<(&str, &str), FrontMatterError> {
    let text = text.trim_start_matches('\u{feff}');
    let mut lines = text.split_inclusive('\n');
    match lines.next() {
        Some(first) if is_delimiter(first) => {}
        _ => return Err(FrontMatterError::Missing),
    }
    let yaml_start = text.find('\n').map_or(text.len(), |i| i + 1);
    let mut offset = yaml_start;
    for line in lines {
        if is_delimiter(line) {
            return Ok((&text[yaml_start..offset], &text[offset + line.len()..]));
        }
        offset += line.len();
    }
    Err(FrontMatterError::Unterminated)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "---\nname: x\nversion: \"1.0.0\"\n---\n# Body\ntext\n";

    #[test]
    fn split_returns_yaml_and_body() {
        let (yaml, body) = split(DOC).unwrap();
        assert_eq!(yaml, "name: x\nversion: \"1.0.0\"\n");
        assert_eq!(body, "# Body\ntext\n");
    }

    #[test]
    fn split_handles_crlf_bom_and_empty_body() {
        let (yaml, body) = split("\u{feff}---\r\nname: x\r\n---\r\n").unwrap();
        assert_eq!(yaml, "name: x\r\n");
        assert_eq!(body, "");
        let (yaml, body) = split("---\n---").unwrap();
        assert_eq!((yaml, body), ("", ""));
    }

    #[test]
    fn split_rejects_missing_or_unterminated() {
        assert!(matches!(split("name: x\n"), Err(FrontMatterError::Missing)));
        assert!(matches!(split(""), Err(FrontMatterError::Missing)));
        assert!(matches!(
            split("---\nname: x\n"),
            Err(FrontMatterError::Unterminated)
        ));
    }

    #[test]
    fn read_front_matter_matches_split() {
        let mut reader = io::Cursor::new(DOC.as_bytes());
        assert_eq!(
            read_front_matter(&mut reader).unwrap(),
            split(DOC).unwrap().0
        );
    }

    #[test]
    fn read_front_matter_stops_before_the_body() {
        let mut bytes = b"---\nname: x\n---\n".to_vec();
        bytes.extend_from_slice(&[0xff, 0xfe, b'\n']);
        let mut reader = io::Cursor::new(bytes);
        assert_eq!(read_front_matter(&mut reader).unwrap(), "name: x\n");
        assert_eq!(reader.position(), 16, "must not consume body bytes");
    }

    #[test]
    fn read_front_matter_errors() {
        let mut r = io::Cursor::new(b"# no front matter\n".as_slice());
        assert!(matches!(
            read_front_matter(&mut r),
            Err(FrontMatterError::Missing)
        ));
        let mut r = io::Cursor::new(b"---\nname: x\n".as_slice());
        assert!(matches!(
            read_front_matter(&mut r),
            Err(FrontMatterError::Unterminated)
        ));
    }
}

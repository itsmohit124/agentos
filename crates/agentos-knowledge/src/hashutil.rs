//! Content fingerprints. Hex-encoded SHA-256, lowercase, no timestamps.

use sha2::{Digest, Sha256};

/// SHA-256 of `bytes` as 64 lowercase hex characters.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Rough token estimate used on fragments: one token per four characters, rounded up.
///
/// This is a length heuristic so estimates are deterministic. It is not a model tokenizer.
pub fn token_estimate(content: &str) -> u32 {
    let chars = content.chars().count();
    u32::try_from(chars.div_ceil(4)).unwrap_or(u32::MAX)
}

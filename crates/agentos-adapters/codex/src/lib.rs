//! AgentOS adapter for Codex.
//!
//! Phase 0 stub. Implemented in Phase 9.

/// Name of this crate.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        assert_eq!(super::CRATE_NAME, "agentos-adapter-codex");
    }
}

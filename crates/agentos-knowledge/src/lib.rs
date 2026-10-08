//! AgentOS knowledge engine: project understanding, knowledge graph and provenance.
//!
//! Phase 0 stub. Implemented in Phase 2.

/// Name of this crate.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        assert_eq!(super::CRATE_NAME, "agentos-knowledge");
    }
}

//! AgentOS context engine: budgeted, traceable Context Packs.
//!
//! Phase 0 stub. Implemented in Phase 4.

/// Name of this crate.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        assert_eq!(super::CRATE_NAME, "agentos-context");
    }
}

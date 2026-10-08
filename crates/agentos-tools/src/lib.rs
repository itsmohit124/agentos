//! AgentOS tools: capability abstractions behind permissions (e.g. browser).
//!
//! Phase 0 stub. Implemented in later phases (browser capability in Phase 8).

/// Name of this crate.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        assert_eq!(super::CRATE_NAME, "agentos-tools");
    }
}

//! AgentOS workflow engine: task graph, planner and scheduler.
//!
//! Phase 0 stub. Implemented in Phase 6.

/// Name of this crate.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        assert_eq!(super::CRATE_NAME, "agentos-workflow");
    }
}

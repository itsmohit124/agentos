//! AgentOS evaluation harness: deterministic fixture, golden and end-to-end evaluations.
//!
//! Phase 0 stub. Evaluation areas are added by each later phase.

/// Name of this crate.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        assert_eq!(super::CRATE_NAME, "agentos-evaluation");
    }
}

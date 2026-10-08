//! AgentOS core.
//!
//! Agent-agnostic and language-agnostic building blocks shared by every subsystem:
//!
//! - [`fs`]: filesystem abstraction ([`fs::FileSystem`]) with a real and an in-memory implementation.
//! - [`git`]: Git detection abstraction ([`git::Git`]) with a CLI-backed and a fake implementation.
//! - [`root`]: project-root detection (`--root` override, Git root, or current directory).
//! - [`state`]: the `.agentos/` project-state layout and its idempotent creation.
//! - [`db`]: the derived SQLite store and its migration runner.

pub mod db;
pub mod fs;
pub mod git;
pub mod root;
pub mod state;

/// Version of the AgentOS core crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

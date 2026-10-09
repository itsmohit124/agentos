//! AgentOS knowledge engine: project understanding, a derived knowledge graph, and provenance.
//!
//! ```text
//! detect(root)                         -> ProjectProfile
//! scan(root)                           -> nodes, edges, fragments in SQLite
//! LanguageExtractor::extract(source)   -> symbols (Python, TypeScript, JavaScript, Go, Java, Ruby)
//! ```
//!
//! The repository stays the source of truth. `agentos.db` and `project-profile.json` are
//! derived: delete them and scan again to reproduce the same graph. Inferred facts are
//! marked `inferred` and are never promoted.

mod assemble;
mod docs;
mod error;
mod extract;
mod hashutil;
mod id;
mod languages;
mod model;
mod profile;
mod scan;
mod store;
mod walk;

#[cfg(test)]
mod tests;

pub use error::KnowledgeError;
pub use extract::{ExtractorRegistry, LanguageExtractor, RawCall, RawSymbol};
pub use model::{
    KnowledgeEdge, KnowledgeFragment, KnowledgeNode, NodeKind, Provenance, ProvenanceKind,
    RelationType,
};
pub use profile::{
    CONFIDENCE_BASE_MILLIS, CONFIDENCE_EMPTY_MILLIS, CONFIDENCE_PER_SIGNAL_MILLIS, Classification,
    Maturity, PROFILE_FILE, ProfileWrite, ProjectProfile, Signal, detect, profile_path,
    read_profile, write_profile,
};
pub use scan::{ScanReport, rebuild, rebuild_with, scan, scan_with};
pub use store::{Inspection, KnowledgeStore, Stats};

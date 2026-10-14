//! Memory records. Files under `.agentos/memory/` are the source of truth.
//! SQLite rows in the same project database mirror those files and use the
//! knowledge provenance fields. `scan` does not delete them.

use std::collections::BTreeMap;

use agentos_knowledge::{Provenance, ProvenanceKind};
use serde::{Deserialize, Serialize};

/// Memory types this phase stores.
pub const MEMORY_TYPES: &[&str] = &["episodic", "semantic", "procedural", "decision", "lesson"];

/// Review states. There is no `canonical` state.
pub const REVIEW_STATUSES: &[&str] = &["proposed", "confirmed", "forgotten"];

/// A fingerprint the record depends on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryLink {
    /// `source`, `domain`, or `symbol`.
    pub kind: String,
    /// Source path or logical node id.
    pub id: String,
    /// Fingerprint at the time the memory was written.
    pub fingerprint: String,
}

impl MemoryLink {
    /// Rejects an empty id, an empty fingerprint, or an unknown kind.
    pub fn validate(&self) -> Result<(), super::error::MemoryError> {
        if !matches!(self.kind.as_str(), "source" | "domain" | "symbol")
            || self.id.is_empty()
            || self.fingerprint.is_empty()
        {
            return Err(super::error::MemoryError::Link(format!(
                "{}:{}:{}",
                self.kind, self.id, self.fingerprint
            )));
        }
        Ok(())
    }
}

/// One persisted memory record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryRecord {
    /// Stable id. Not a path.
    pub id: String,
    /// One of [`MEMORY_TYPES`].
    pub memory_type: String,
    /// Short name.
    pub name: String,
    /// Redacted body.
    pub content: String,
    /// Structured fields. Keys are sorted.
    pub payload: BTreeMap<String, String>,
    /// `proposed`, `confirmed`, or `forgotten`. Never `canonical`.
    pub status: String,
    /// `fresh` or `stale`.
    pub freshness: String,
    /// Why the record exists. Uses the knowledge provenance model.
    pub provenance: Provenance,
    /// Confidence in thousandths. Not a calibrated probability.
    pub confidence_millis: u16,
    /// SHA-256 of `content`.
    pub fingerprint: String,
    /// Task text, redacted.
    pub task: String,
    /// Run id, or empty for a record that did not come from a run.
    pub run_id: String,
    /// Outcome such as `failed` or `completed`. Empty when unknown.
    pub outcome: String,
    /// Fingerprints this record depends on, sorted by kind then id.
    pub links: Vec<MemoryLink>,
    /// Caller-supplied creation time. Not generated inside the library.
    pub created_at: String,
    /// Caller-supplied update time.
    pub updated_at: String,
    /// True when this record is a successful context-pack shape.
    pub pattern: bool,
    /// Intent kind for a pattern. Empty otherwise.
    pub intent_kind: String,
}

impl MemoryRecord {
    /// True when retrieval must ignore the record.
    pub fn is_forgotten(&self) -> bool {
        self.status == "forgotten"
    }

    /// True when the review state is `canonical`. Always false.
    pub fn is_canonical(&self) -> bool {
        false
    }
}

/// Fields for [`super::MemoryStore::add`].
#[derive(Debug, Clone)]
pub struct NewMemory {
    /// Memory type.
    pub memory_type: String,
    /// Short name.
    pub name: String,
    /// Body, redacted on write.
    pub content: String,
    /// Task, redacted on write.
    pub task: String,
    /// Run id.
    pub run_id: String,
    /// Outcome.
    pub outcome: String,
    /// Fingerprint links.
    pub links: Vec<MemoryLink>,
    /// Timestamp supplied by the caller.
    pub recorded_at: String,
    /// When true, the new record is `confirmed`. Otherwise it is `proposed`.
    pub confirm: bool,
    /// Successful context pattern.
    pub pattern: bool,
    /// Intent kind for a pattern.
    pub intent_kind: String,
    /// Extra structured fields. Values are redacted.
    pub payload: BTreeMap<String, String>,
    /// Provenance kind.
    pub provenance_kind: ProvenanceKind,
    /// Provenance method.
    pub method: String,
    /// Confidence in thousandths.
    pub confidence_millis: u16,
}

/// Current fingerprints used to decide staleness.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FingerprintIndex {
    /// Id or path to fingerprint.
    pub values: BTreeMap<String, String>,
}

impl FingerprintIndex {
    /// An index with no entries. Linked memories become stale.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts one fingerprint.
    pub fn insert(&mut self, id: impl Into<String>, fingerprint: impl Into<String>) {
        self.values.insert(id.into(), fingerprint.into());
    }

    /// Reads source-file and node fingerprints from the knowledge index.
    pub fn from_store(
        store: &agentos_knowledge::KnowledgeStore,
    ) -> Result<Self, super::error::MemoryError> {
        let mut values = BTreeMap::new();
        for (path, fingerprint) in store.fingerprints()? {
            values.insert(path, fingerprint);
        }
        for node in store.nodes()? {
            values.insert(node.id, node.fingerprint);
        }
        Ok(Self { values })
    }
}

/// Parses a memory type.
pub fn parse_type(value: &str) -> Result<&str, super::error::MemoryError> {
    MEMORY_TYPES
        .iter()
        .find(|item| **item == value)
        .copied()
        .ok_or_else(|| super::error::MemoryError::Type(value.to_owned()))
}

/// `canonical` and any unknown review state become `proposed`.
pub fn normalize_status(status: &str, freshness: &str) -> (String, String, bool) {
    if status == "stale" {
        return ("proposed".to_owned(), "stale".to_owned(), true);
    }
    let mut changed = false;
    let review = match status {
        "confirmed" => "confirmed",
        "forgotten" => "forgotten",
        "proposed" => "proposed",
        _ => {
            changed = true;
            "proposed"
        }
    };
    let fresh = match freshness {
        "stale" => "stale",
        "fresh" => "fresh",
        _ => {
            changed = true;
            "fresh"
        }
    };
    (review.to_owned(), fresh.to_owned(), changed)
}

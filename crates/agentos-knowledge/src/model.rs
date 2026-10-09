//! Knowledge graph records.
//!
//! A node is what exists, an edge is how two nodes relate, and a fragment is a slice of
//! source that can later be shown to an agent. Every node and edge carries provenance.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::hashutil::{sha256_hex, token_estimate};

/// Kind of a knowledge node extracted in this phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// A source file.
    File,
    /// A language module (Python module, TS module).
    Module,
    /// A class.
    Class,
    /// An interface (TypeScript `interface`).
    Interface,
    /// A module-level function.
    Function,
    /// A function defined on a class or interface.
    Method,
    /// An import statement.
    Import,
    /// A test function or test method.
    Test,
    /// A named symbol that is not one of the kinds above (constant, type alias).
    Symbol,
    /// A markdown document.
    Document,
    /// An architecture decision record.
    Decision,
    /// A bounded context (Phase 3). Stored in `domain_nodes`.
    BoundedContext,
    /// A ubiquitous-language term.
    Terminology,
    /// A domain entity.
    Entity,
    /// A value object.
    ValueObject,
    /// An aggregate or aggregate root.
    Aggregate,
    /// A business invariant.
    Invariant,
    /// A domain service.
    DomainService,
    /// A domain repository. Software classes stay `class`.
    Repository,
    /// A domain policy, including a corrective policy.
    Policy,
    /// A domain event.
    DomainEvent,
    /// An anti-corruption layer.
    AntiCorruptionLayer,
}

impl NodeKind {
    /// Stable wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Module => "module",
            Self::Class => "class",
            Self::Interface => "interface",
            Self::Function => "function",
            Self::Method => "method",
            Self::Import => "import",
            Self::Test => "test",
            Self::Symbol => "symbol",
            Self::Document => "document",
            Self::Decision => "decision",
            Self::BoundedContext => "bounded_context",
            Self::Terminology => "terminology",
            Self::Entity => "entity",
            Self::ValueObject => "value_object",
            Self::Aggregate => "aggregate",
            Self::Invariant => "invariant",
            Self::DomainService => "domain_service",
            Self::Repository => "repository",
            Self::Policy => "policy",
            Self::DomainEvent => "domain_event",
            Self::AntiCorruptionLayer => "anti_corruption_layer",
        }
    }

    /// Parses a wire name.
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "file" => Self::File,
            "module" => Self::Module,
            "class" => Self::Class,
            "interface" => Self::Interface,
            "function" => Self::Function,
            "method" => Self::Method,
            "import" => Self::Import,
            "test" => Self::Test,
            "symbol" => Self::Symbol,
            "document" => Self::Document,
            "decision" => Self::Decision,
            "bounded_context" => Self::BoundedContext,
            "terminology" => Self::Terminology,
            "entity" => Self::Entity,
            "value_object" => Self::ValueObject,
            "aggregate" => Self::Aggregate,
            "invariant" => Self::Invariant,
            "domain_service" => Self::DomainService,
            "repository" => Self::Repository,
            "policy" => Self::Policy,
            "domain_event" => Self::DomainEvent,
            "anti_corruption_layer" => Self::AntiCorruptionLayer,
            _ => return None,
        })
    }

    /// True for Phase 3 domain concepts. Software nodes stay false.
    pub fn is_domain(self) -> bool {
        matches!(
            self,
            Self::BoundedContext
                | Self::Terminology
                | Self::Entity
                | Self::ValueObject
                | Self::Aggregate
                | Self::Invariant
                | Self::DomainService
                | Self::Repository
                | Self::Policy
                | Self::DomainEvent
                | Self::AntiCorruptionLayer
        )
    }
}

/// Relationship stored on an edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RelationType {
    /// The source contains the target (file→module, module→symbol, class→method).
    Contains,
    /// The source module imports the target module.
    Imports,
    /// The source calls the target.
    Calls,
    /// The source test exercises the target.
    Tests,
    /// The source type extends the target type.
    Extends,
    /// The source type implements the target interface.
    Implements,
    /// The source mentions the target. Inferred mentions stay inferred.
    References,
    /// The source domain concept belongs to the target context.
    BelongsTo,
    /// The context has the entity.
    HasEntity,
    /// The source has the value object.
    HasValueObject,
    /// The context has the aggregate.
    HasAggregate,
    /// The source is constrained by the target invariant.
    ConstrainedBy,
    /// The source policy governs the target.
    Governs,
    /// The source context calls into the target context.
    CrossContextTo,
    /// The source anti-corruption layer protects the target context.
    AntiCorrupts,
    /// The source concept was read from the target node.
    DerivedFrom,
    /// The source term conflicts with the target term. Not a merge.
    ConflictsWith,
}

impl RelationType {
    /// Stable wire name (`CONTAINS`, `CALLS`, …).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Contains => "CONTAINS",
            Self::Imports => "IMPORTS",
            Self::Calls => "CALLS",
            Self::Tests => "TESTS",
            Self::Extends => "EXTENDS",
            Self::Implements => "IMPLEMENTS",
            Self::References => "REFERENCES",
            Self::BelongsTo => "BELONGS_TO",
            Self::HasEntity => "HAS_ENTITY",
            Self::HasValueObject => "HAS_VALUE_OBJECT",
            Self::HasAggregate => "HAS_AGGREGATE",
            Self::ConstrainedBy => "CONSTRAINED_BY",
            Self::Governs => "GOVERNS",
            Self::CrossContextTo => "CROSS_CONTEXT_TO",
            Self::AntiCorrupts => "ANTI_CORRUPTS",
            Self::DerivedFrom => "DERIVED_FROM",
            Self::ConflictsWith => "CONFLICTS_WITH",
        }
    }

    /// Parses a wire name.
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "CONTAINS" => Self::Contains,
            "IMPORTS" => Self::Imports,
            "CALLS" => Self::Calls,
            "TESTS" => Self::Tests,
            "EXTENDS" => Self::Extends,
            "IMPLEMENTS" => Self::Implements,
            "REFERENCES" => Self::References,
            "BELONGS_TO" => Self::BelongsTo,
            "HAS_ENTITY" => Self::HasEntity,
            "HAS_VALUE_OBJECT" => Self::HasValueObject,
            "HAS_AGGREGATE" => Self::HasAggregate,
            "CONSTRAINED_BY" => Self::ConstrainedBy,
            "GOVERNS" => Self::Governs,
            "CROSS_CONTEXT_TO" => Self::CrossContextTo,
            "ANTI_CORRUPTS" => Self::AntiCorrupts,
            "DERIVED_FROM" => Self::DerivedFrom,
            "CONFLICTS_WITH" => Self::ConflictsWith,
            _ => return None,
        })
    }
}

/// Why a fact is in the graph.
///
/// `inferred` is never rewritten to another kind. Promotion is a human act.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProvenanceKind {
    /// Read from the repository or from a tool that reads the repository.
    Observed,
    /// Written by a person in a project artifact (a spec, an ADR, a docstring file).
    Declared,
    /// Produced by a heuristic (for example a name mentioned in a document).
    Inferred,
    /// Produced by AgentOS itself (the project profile).
    Generated,
}

impl ProvenanceKind {
    /// Stable wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Observed => "observed",
            Self::Declared => "declared",
            Self::Inferred => "inferred",
            Self::Generated => "generated",
        }
    }

    /// Parses a wire name.
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "observed" => Self::Observed,
            "declared" => Self::Declared,
            "inferred" => Self::Inferred,
            "generated" => Self::Generated,
            _ => return None,
        })
    }
}

/// Where a fact came from and how it was obtained.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    /// Observed, declared, inferred, or generated.
    pub kind: ProvenanceKind,
    /// Relative path of the source artifact, or empty for a whole-project generated fact.
    pub source: String,
    /// Location inside the source, `L<start>-L<end>` (1-based lines), or empty.
    pub location: String,
    /// Extractor that produced the fact (`python`, `markdown`, `profile`, …).
    pub extractor: String,
    /// Extractor version. Bumped only when the extractor's output rules change.
    pub extractor_version: String,
    /// How the fact was obtained (`tree-sitter`, `markdown-headings`, `name-mention`, …).
    pub method: String,
}

impl Provenance {
    /// True when every field needed to audit the fact is present.
    pub fn is_complete(&self) -> bool {
        !self.extractor.is_empty() && !self.extractor_version.is_empty() && !self.method.is_empty()
    }
}

/// One node in the derived knowledge graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeNode {
    /// Stable logical id. Not a file path.
    pub id: String,
    /// What kind of thing this is.
    pub node_type: NodeKind,
    /// Local name (`PaymentService`, `charge`, `overview`).
    pub name: String,
    /// Qualified module, or the document path stem.
    pub scope: String,
    /// One-line description taken from a docstring, heading, or signature.
    pub summary: String,
    /// Lifecycle status. Extracted facts are `active`.
    pub status: String,
    /// Why this node is believed.
    pub provenance: Provenance,
    /// Confidence in thousandths (0–1000).
    pub confidence_millis: u16,
    /// Schema version of this node. Always 1 in this phase (the index has no history).
    pub version: u32,
    /// Fingerprint of the node's identity and body. Changes when the body changes.
    pub fingerprint: String,
    /// Relative paths that support the node, sorted.
    pub source_refs: Vec<String>,
    /// Reserved for later retrieval. Empty in this phase.
    pub retrieval_metadata: BTreeMap<String, String>,
    /// Extractor-specific attributes (bases, import target), keys sorted.
    pub payload: BTreeMap<String, String>,
    /// Relative path of the file this node was extracted from.
    pub source_id: String,
    /// Enclosing type name for methods.
    pub parent: Option<String>,
}

/// One directed relationship.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeEdge {
    /// Stable id derived from the endpoints and the relation.
    pub id: String,
    /// Source node id.
    pub from_id: String,
    /// Relation.
    pub relation: RelationType,
    /// Target node id.
    pub to_id: String,
    /// Confidence in thousandths.
    pub confidence_millis: u16,
    /// Why this edge is believed.
    pub provenance: Provenance,
    /// Extra attributes. Empty unless an extractor has something to record.
    pub metadata: BTreeMap<String, String>,
    /// Relative path of the file that asserted the edge.
    pub source_id: String,
}

/// A retrievable slice of a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeFragment {
    /// `{node_id}#{index}`.
    pub id: String,
    /// Node this slice belongs to.
    pub node_id: String,
    /// Relative path of the file.
    pub source_ref: String,
    /// `L<start>-L<end>`.
    pub location: String,
    /// Exact slice text.
    pub content: String,
    /// Fingerprint of `content`.
    pub fingerprint: String,
    /// Deterministic length heuristic. See [`token_estimate`].
    pub token_estimate: u32,
    /// `code`, `import`, or `section`.
    pub kind: String,
}

/// Current node version written by this phase. The index is derived and has no history.
pub const NODE_VERSION: u32 = 1;

/// Structural facts read directly from a parsed tree.
pub const CONFIDENCE_STRUCTURAL: u16 = 900;

/// Facts resolved from an annotation or import that is written in the source.
pub const CONFIDENCE_RESOLVED: u16 = 850;

/// Name mentions in documents. These stay [`ProvenanceKind::Inferred`].
pub const CONFIDENCE_INFERRED: u16 = 400;

/// A human-written document or decision, taken as declared.
pub const CONFIDENCE_DECLARED: u16 = 1000;

/// Builds a node fingerprint from the fields that should change when its text changes.
pub fn node_fingerprint(
    kind: NodeKind,
    name: &str,
    scope: &str,
    provenance: ProvenanceKind,
    body: &str,
) -> String {
    sha256_hex(
        format!(
            "{}\n{name}\n{scope}\n{}\n{body}",
            kind.as_str(),
            provenance.as_str()
        )
        .as_bytes(),
    )
}

/// Builds a fragment from a source slice.
pub fn fragment(
    node_id: &str,
    index: u32,
    source_ref: &str,
    location: &str,
    content: &str,
    kind: &str,
) -> KnowledgeFragment {
    KnowledgeFragment {
        id: format!("{node_id}#{index}"),
        node_id: node_id.to_owned(),
        source_ref: source_ref.to_owned(),
        location: location.to_owned(),
        content: content.to_owned(),
        fingerprint: sha256_hex(content.as_bytes()),
        token_estimate: token_estimate(content),
        kind: kind.to_owned(),
    }
}

/// `L{start}-L{end}`.
pub fn line_span(start: u32, end: u32) -> String {
    let end = end.max(start);
    format!("L{start}-L{end}")
}

/// Edge id. The separator cannot appear in a relation name.
pub fn edge_id(from: &str, relation: RelationType, to: &str) -> String {
    format!("{from}|{}|{to}", relation.as_str())
}

/// A domain finding. Ambiguity and leaks are findings, not merges and not canonical facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// Stable id.
    pub id: String,
    /// `terminology_ambiguity`, `hexagonal_leak`, `aggregate_boundary`, or `drift`.
    pub kind: String,
    /// One sentence.
    pub summary: String,
    /// Why the finding is believed.
    pub provenance: Provenance,
    /// Confidence in thousandths.
    pub confidence_millis: u16,
    /// Sorted evidence paths or quotes.
    pub evidence: String,
    /// Related node ids, sorted.
    pub subjects: Vec<String>,
}

/// One recorded lifecycle change. `occurred_at` is audit data and is not written to golden files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransitionRecord {
    /// `{node_id}|{from}|{to}|{index}`.
    pub id: String,
    /// Domain node id.
    pub node_id: String,
    /// Previous status, or `none` when the node is created.
    pub from_status: String,
    /// Status after the change.
    pub to_status: String,
    /// Who asked for the change (`agentos`, `operator`, a proposer name).
    pub actor: String,
    /// Why the change was made. Never empty.
    pub reason: String,
    /// Clock value. Omitted from generated markdown.
    pub occurred_at: String,
}

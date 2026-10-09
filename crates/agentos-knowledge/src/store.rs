//! Derived SQLite index for the knowledge graph.
//!
//! Migrations live here, not in `agentos-core`, because core cannot depend on this crate.
//! The core database still opens first (its `meta` table); this store then applies
//! `knowledge_migrations`. Deleting `agentos.db` and scanning again reproduces the graph.

use std::collections::BTreeMap;
use std::path::Path;

use agentos_core::db::Database;
use agentos_core::state::{DB_FILE, STATE_DIR};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::assemble::{Draft, StoredSymbol};
use crate::error::KnowledgeError;
use crate::model::{
    Finding, KnowledgeEdge, KnowledgeFragment, KnowledgeNode, NodeKind, Provenance, ProvenanceKind,
    RelationType, TransitionRecord,
};

const MIGRATION_SQL: &str = "\
CREATE TABLE sources (
    id TEXT PRIMARY KEY NOT NULL,
    fingerprint TEXT NOT NULL,
    language TEXT NOT NULL,
    byte_len INTEGER NOT NULL
) STRICT;
CREATE TABLE nodes (
    id TEXT PRIMARY KEY NOT NULL,
    node_type TEXT NOT NULL,
    name TEXT NOT NULL,
    scope TEXT NOT NULL,
    summary TEXT NOT NULL,
    status TEXT NOT NULL,
    provenance_kind TEXT NOT NULL,
    provenance_source TEXT NOT NULL,
    provenance_location TEXT NOT NULL,
    extractor TEXT NOT NULL,
    extractor_version TEXT NOT NULL,
    method TEXT NOT NULL,
    confidence_millis INTEGER NOT NULL,
    version INTEGER NOT NULL,
    fingerprint TEXT NOT NULL,
    source_refs TEXT NOT NULL,
    retrieval_metadata TEXT NOT NULL,
    payload TEXT NOT NULL,
    source_id TEXT NOT NULL,
    parent TEXT,
    FOREIGN KEY (source_id) REFERENCES sources(id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE edges (
    id TEXT PRIMARY KEY NOT NULL,
    from_id TEXT NOT NULL,
    relation_type TEXT NOT NULL,
    to_id TEXT NOT NULL,
    confidence_millis INTEGER NOT NULL,
    provenance_kind TEXT NOT NULL,
    provenance_source TEXT NOT NULL,
    provenance_location TEXT NOT NULL,
    extractor TEXT NOT NULL,
    extractor_version TEXT NOT NULL,
    method TEXT NOT NULL,
    metadata TEXT NOT NULL,
    source_id TEXT NOT NULL,
    FOREIGN KEY (from_id) REFERENCES nodes(id) DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (to_id) REFERENCES nodes(id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE fragments (
    id TEXT PRIMARY KEY NOT NULL,
    node_id TEXT NOT NULL,
    source_ref TEXT NOT NULL,
    location TEXT NOT NULL,
    content TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    token_estimate INTEGER NOT NULL,
    kind TEXT NOT NULL,
    FOREIGN KEY (node_id) REFERENCES nodes(id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE INDEX idx_nodes_source ON nodes(source_id);
CREATE INDEX idx_nodes_type ON nodes(node_type);
CREATE INDEX idx_edges_from ON edges(from_id);
CREATE INDEX idx_edges_to ON edges(to_id);
CREATE INDEX idx_edges_source ON edges(source_id);
CREATE INDEX idx_fragments_node ON fragments(node_id);
CREATE VIRTUAL TABLE fragments_fts USING fts5(
    id UNINDEXED,
    name,
    content,
    tokenize = 'unicode61'
);
";

/// Phase 3 tables. Additive: version 1 tables are not rewritten.
const MIGRATION_V2_SQL: &str = "\
CREATE TABLE domain_nodes (
    id TEXT PRIMARY KEY NOT NULL,
    node_type TEXT NOT NULL,
    name TEXT NOT NULL,
    scope TEXT NOT NULL,
    summary TEXT NOT NULL,
    status TEXT NOT NULL,
    provenance_kind TEXT NOT NULL,
    provenance_source TEXT NOT NULL,
    provenance_location TEXT NOT NULL,
    extractor TEXT NOT NULL,
    extractor_version TEXT NOT NULL,
    method TEXT NOT NULL,
    confidence_millis INTEGER NOT NULL,
    version INTEGER NOT NULL,
    fingerprint TEXT NOT NULL,
    source_refs TEXT NOT NULL,
    retrieval_metadata TEXT NOT NULL,
    payload TEXT NOT NULL,
    source_id TEXT NOT NULL,
    parent TEXT
) STRICT;
CREATE TABLE domain_edges (
    id TEXT PRIMARY KEY NOT NULL,
    from_id TEXT NOT NULL,
    relation_type TEXT NOT NULL,
    to_id TEXT NOT NULL,
    confidence_millis INTEGER NOT NULL,
    provenance_kind TEXT NOT NULL,
    provenance_source TEXT NOT NULL,
    provenance_location TEXT NOT NULL,
    extractor TEXT NOT NULL,
    extractor_version TEXT NOT NULL,
    method TEXT NOT NULL,
    metadata TEXT NOT NULL,
    source_id TEXT NOT NULL
) STRICT;
CREATE TABLE domain_transitions (
    id TEXT PRIMARY KEY NOT NULL,
    node_id TEXT NOT NULL,
    from_status TEXT NOT NULL,
    to_status TEXT NOT NULL,
    actor TEXT NOT NULL,
    reason TEXT NOT NULL,
    occurred_at TEXT NOT NULL
) STRICT;
CREATE TABLE domain_findings (
    id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL,
    summary TEXT NOT NULL,
    provenance_kind TEXT NOT NULL,
    provenance_source TEXT NOT NULL,
    provenance_location TEXT NOT NULL,
    extractor TEXT NOT NULL,
    extractor_version TEXT NOT NULL,
    method TEXT NOT NULL,
    confidence_millis INTEGER NOT NULL,
    evidence TEXT NOT NULL,
    subjects TEXT NOT NULL
) STRICT;
CREATE INDEX idx_domain_nodes_type ON domain_nodes(node_type);
CREATE INDEX idx_domain_edges_from ON domain_edges(from_id);
CREATE INDEX idx_domain_edges_to ON domain_edges(to_id);
CREATE INDEX idx_domain_transitions_node ON domain_transitions(node_id);
";

/// Open derived store for one project root.
pub struct KnowledgeStore {
    db: Database,
}

/// Counts grouped for `knowledge stats`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Stats {
    /// Fingerprinted files.
    pub files: usize,
    /// Nodes.
    pub nodes: usize,
    /// Edges.
    pub edges: usize,
    /// Fragments.
    pub fragments: usize,
    /// Node counts by type name.
    pub nodes_by_type: BTreeMap<String, usize>,
    /// Edge counts by relation.
    pub edges_by_type: BTreeMap<String, usize>,
    /// Node and edge counts by provenance kind.
    pub provenance: BTreeMap<String, usize>,
    /// Knowledge schema version.
    pub schema_version: u32,
}

/// One node plus the edges and fragments attached to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Inspection {
    /// The node.
    pub node: KnowledgeNode,
    /// Edges whose `from` is this node, sorted by id.
    pub outgoing: Vec<KnowledgeEdge>,
    /// Edges whose `to` is this node, sorted by id.
    pub incoming: Vec<KnowledgeEdge>,
    /// Fragments of this node, sorted by id.
    pub fragments: Vec<KnowledgeFragment>,
}

impl KnowledgeStore {
    /// Opens (and migrates) `<root>/.agentos/agentos.db`.
    pub fn open(root: &Path) -> Result<Self, KnowledgeError> {
        let dir = root.join(STATE_DIR);
        std::fs::create_dir_all(&dir)?;
        let (db, _) = Database::open(&dir.join(DB_FILE))?;
        migrate(db.connection())?;
        Ok(Self { db })
    }

    fn conn(&self) -> &Connection {
        self.db.connection()
    }

    /// Knowledge schema version.
    pub fn schema_version(&self) -> Result<u32, KnowledgeError> {
        Ok(self.conn().query_row(
            "SELECT COALESCE(MAX(version), 0) FROM knowledge_migrations",
            [],
            |row| row.get(0),
        )?)
    }

    /// Relative path → content fingerprint.
    pub fn fingerprints(&self) -> Result<BTreeMap<String, String>, KnowledgeError> {
        let mut stmt = self
            .conn()
            .prepare("SELECT id, fingerprint FROM sources ORDER BY id")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<Result<BTreeMap<_, _>, _>>()
            .map_err(KnowledgeError::from)
    }

    /// Symbols currently stored, optionally skipping some source paths.
    pub fn symbols_except(
        &self,
        skip: &BTreeMap<String, ()>,
    ) -> Result<Vec<StoredSymbol>, KnowledgeError> {
        let mut stmt = self.conn().prepare(
            "SELECT id, node_type, name, parent, scope, source_id FROM nodes ORDER BY id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, kind, name, parent, scope, source) = row?;
            if skip.contains_key(&source) {
                continue;
            }
            let Some(kind) = NodeKind::parse(&kind) else {
                continue;
            };
            out.push(StoredSymbol {
                id,
                kind,
                name,
                parent,
                scope,
            });
        }
        Ok(out)
    }

    /// Class, interface, and function names with their ids. Sorted.
    pub fn mention_targets(&self) -> Result<Vec<StoredSymbol>, KnowledgeError> {
        let symbols = self.symbols_except(&BTreeMap::new())?;
        Ok(symbols
            .into_iter()
            .filter(|symbol| {
                matches!(
                    symbol.kind,
                    NodeKind::Class | NodeKind::Interface | NodeKind::Function
                )
            })
            .collect())
    }

    /// Applies one incremental update: delete sources, write drafts, fix dangling edges,
    /// optionally replace inferred mentions, and rebuild FTS. Returns rows written.
    pub fn apply_changes(
        &self,
        deleted: &[String],
        drafts: &[Draft],
        mentions: Option<&[KnowledgeEdge]>,
    ) -> Result<u64, KnowledgeError> {
        let before = self.total_changes();
        let tx = self.conn().unchecked_transaction()?;
        tx.execute_batch("PRAGMA defer_foreign_keys = ON;")?;
        for source in deleted {
            delete_source_sql(&tx, source)?;
        }
        for draft in drafts {
            delete_source_sql(&tx, &draft.source_id)?;
            insert_draft_sql(&tx, draft)?;
        }
        delete_dangling_sql(&tx)?;
        if let Some(edges) = mentions {
            tx.execute("DELETE FROM edges WHERE method = 'name-mention'", [])?;
            for edge in edges {
                insert_edge_sql(&tx, edge)?;
            }
        }
        tx.execute("DELETE FROM fragments_fts", [])?;
        tx.execute(
            "INSERT INTO fragments_fts (id, name, content)
             SELECT f.id, n.name, f.content FROM fragments f JOIN nodes n ON n.id = f.node_id",
            [],
        )?;
        tx.commit()?;
        Ok(self.total_changes() - before)
    }

    /// Deletes every derived row, including domain rows. The schema stays.
    ///
    /// Transition history is derived with the rest of the index. Deleting `agentos.db`
    /// also drops confirmations.
    pub fn clear(&self) -> Result<(), KnowledgeError> {
        self.conn().execute_batch(
            "DELETE FROM domain_findings;
             DELETE FROM domain_transitions;
             DELETE FROM domain_edges;
             DELETE FROM domain_nodes;
             DELETE FROM fragments_fts;
             DELETE FROM fragments;
             DELETE FROM edges;
             DELETE FROM nodes;
             DELETE FROM sources;",
        )?;
        Ok(())
    }

    /// Current name-mention edges, sorted by id.
    pub fn mentions(&self) -> Result<Vec<KnowledgeEdge>, KnowledgeError> {
        self.edges_where("method = 'name-mention'")
    }

    /// Fragment ids whose name or content matches an FTS query.
    pub fn search(&self, query: &str) -> Result<Vec<String>, KnowledgeError> {
        let mut stmt = self
            .conn()
            .prepare("SELECT id FROM fragments_fts WHERE fragments_fts MATCH ?1 ORDER BY id")?;
        let rows = stmt.query_map(params![query], |row| row.get(0))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(KnowledgeError::from)
    }

    /// All fragments, sorted by id.
    pub fn fragments(&self) -> Result<Vec<KnowledgeFragment>, KnowledgeError> {
        let mut stmt = self.conn().prepare("SELECT * FROM fragments ORDER BY id")?;
        let rows = stmt.query_map([], fragment_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(KnowledgeError::from)
    }

    /// Software nodes only, sorted by id. Domain rows live in [`Self::domain_nodes`].
    pub(crate) fn software_nodes(&self) -> Result<Vec<KnowledgeNode>, KnowledgeError> {
        table_nodes(self.conn(), "nodes")
    }

    /// Domain nodes only, sorted by id.
    pub fn domain_nodes(&self) -> Result<Vec<KnowledgeNode>, KnowledgeError> {
        table_nodes(self.conn(), "domain_nodes")
    }

    /// Software and domain nodes, sorted by id.
    pub fn nodes(&self) -> Result<Vec<KnowledgeNode>, KnowledgeError> {
        let mut all = self.software_nodes()?;
        all.extend(self.domain_nodes()?);
        all.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(all)
    }

    /// Software edges only, sorted by id.
    pub(crate) fn software_edges(&self) -> Result<Vec<KnowledgeEdge>, KnowledgeError> {
        table_edges(self.conn(), "edges")
    }

    /// Domain edges only, sorted by id.
    pub fn domain_edges(&self) -> Result<Vec<KnowledgeEdge>, KnowledgeError> {
        table_edges(self.conn(), "domain_edges")
    }

    /// Software and domain edges, sorted by id.
    pub fn edges(&self) -> Result<Vec<KnowledgeEdge>, KnowledgeError> {
        let mut all = self.software_edges()?;
        all.extend(self.domain_edges()?);
        all.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(all)
    }

    /// Stored findings, sorted by id.
    pub fn domain_findings(&self) -> Result<Vec<Finding>, KnowledgeError> {
        let mut stmt = self
            .conn()
            .prepare("SELECT * FROM domain_findings ORDER BY id")?;
        let rows = stmt.query_map([], finding_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(KnowledgeError::from)
    }

    /// Transitions for one node, in insertion order.
    pub(crate) fn transitions_for(
        &self,
        node_id: &str,
    ) -> Result<Vec<TransitionRecord>, KnowledgeError> {
        let mut stmt = self
            .conn()
            .prepare("SELECT * FROM domain_transitions WHERE node_id = ?1 ORDER BY id")?;
        let rows = stmt.query_map(params![node_id], transition_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(KnowledgeError::from)
    }

    /// Replaces domain rows only when the new set differs. Appends transitions.
    /// Returns SQLite rows written.
    pub(crate) fn sync_domain(
        &self,
        nodes: &[KnowledgeNode],
        edges: &[KnowledgeEdge],
        findings: &[Finding],
        transitions: &[TransitionRecord],
    ) -> Result<u64, KnowledgeError> {
        let before = self.total_changes();
        let tx = self.conn().unchecked_transaction()?;
        let existing_nodes = table_nodes(&tx, "domain_nodes")?;
        if existing_nodes != nodes {
            tx.execute("DELETE FROM domain_nodes", [])?;
            for node in nodes {
                insert_node_into(&tx, "domain_nodes", node)?;
            }
        }
        let existing_edges = table_edges(&tx, "domain_edges")?;
        if existing_edges != edges {
            tx.execute("DELETE FROM domain_edges", [])?;
            for edge in edges {
                insert_edge_into(&tx, "domain_edges", edge)?;
            }
        }
        let existing_findings = {
            let mut stmt = tx.prepare("SELECT * FROM domain_findings ORDER BY id")?;
            let rows = stmt.query_map([], finding_from_row)?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        if existing_findings != findings {
            tx.execute("DELETE FROM domain_findings", [])?;
            for finding in findings {
                insert_finding(&tx, finding)?;
            }
        }
        for transition in transitions {
            insert_transition(&tx, transition)?;
        }
        tx.commit()?;
        Ok(self.total_changes() - before)
    }

    /// Aggregated counts, including domain rows.
    pub fn stats(&self) -> Result<Stats, KnowledgeError> {
        let files = count(self.conn(), "SELECT COUNT(*) FROM sources")?;
        let nodes = count(self.conn(), "SELECT COUNT(*) FROM nodes")?
            + count(self.conn(), "SELECT COUNT(*) FROM domain_nodes")?;
        let edges = count(self.conn(), "SELECT COUNT(*) FROM edges")?
            + count(self.conn(), "SELECT COUNT(*) FROM domain_edges")?;
        let fragments = count(self.conn(), "SELECT COUNT(*) FROM fragments")?;
        Ok(Stats {
            files,
            nodes,
            edges,
            fragments,
            nodes_by_type: grouped_merge(
                self.conn(),
                "SELECT node_type, COUNT(*) FROM nodes GROUP BY node_type",
                "SELECT node_type, COUNT(*) FROM domain_nodes GROUP BY node_type",
            )?,
            edges_by_type: grouped_merge(
                self.conn(),
                "SELECT relation_type, COUNT(*) FROM edges GROUP BY relation_type",
                "SELECT relation_type, COUNT(*) FROM domain_edges GROUP BY relation_type",
            )?,
            provenance: provenance_counts(self.conn())?,
            schema_version: self.schema_version()?,
        })
    }

    /// One software or domain node, with edges from both tables and software fragments.
    pub fn inspect(&self, id: &str) -> Result<Inspection, KnowledgeError> {
        let node = if let Some(node) = self
            .conn()
            .query_row(
                "SELECT * FROM nodes WHERE id = ?1",
                params![id],
                node_from_row,
            )
            .optional()?
        {
            node
        } else if let Some(node) = self
            .conn()
            .query_row(
                "SELECT * FROM domain_nodes WHERE id = ?1",
                params![id],
                node_from_row,
            )
            .optional()?
        {
            node
        } else {
            return Err(KnowledgeError::NotFound(id.to_owned()));
        };
        let mut outgoing = self.edges_where_param("edges", "from_id = ?1", id)?;
        outgoing.extend(self.edges_where_param("domain_edges", "from_id = ?1", id)?);
        outgoing.sort_by(|a, b| a.id.cmp(&b.id));
        let mut incoming = self.edges_where_param("edges", "to_id = ?1", id)?;
        incoming.extend(self.edges_where_param("domain_edges", "to_id = ?1", id)?);
        incoming.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(Inspection {
            outgoing,
            incoming,
            fragments: self.fragments_of(id)?,
            node,
        })
    }

    /// True when every node and edge has a complete provenance record of a known kind.
    pub fn provenance_complete(&self) -> Result<bool, KnowledgeError> {
        let nodes = self.nodes()?;
        let edges = self.edges()?;
        Ok(nodes.iter().all(|node| node.provenance.is_complete())
            && edges.iter().all(|edge| edge.provenance.is_complete()))
    }

    /// Rows changed on this connection so far.
    pub fn total_changes(&self) -> u64 {
        self.conn().total_changes()
    }

    fn edges_where(&self, clause: &str) -> Result<Vec<KnowledgeEdge>, KnowledgeError> {
        let sql = format!("SELECT * FROM edges WHERE {clause} ORDER BY id");
        let mut stmt = self.conn().prepare(&sql)?;
        let rows = stmt.query_map([], edge_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(KnowledgeError::from)
    }

    fn edges_where_param(
        &self,
        table: &str,
        clause: &str,
        id: &str,
    ) -> Result<Vec<KnowledgeEdge>, KnowledgeError> {
        let sql = format!("SELECT * FROM {table} WHERE {clause} ORDER BY id");
        let mut stmt = self.conn().prepare(&sql)?;
        let rows = stmt.query_map(params![id], edge_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(KnowledgeError::from)
    }

    fn fragments_of(&self, id: &str) -> Result<Vec<KnowledgeFragment>, KnowledgeError> {
        let mut stmt = self
            .conn()
            .prepare("SELECT * FROM fragments WHERE node_id = ?1 ORDER BY id")?;
        let rows = stmt.query_map(params![id], fragment_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(KnowledgeError::from)
    }
}

fn delete_source_sql(conn: &Connection, source: &str) -> Result<(), KnowledgeError> {
    conn.execute(
        "DELETE FROM fragments WHERE node_id IN (SELECT id FROM nodes WHERE source_id = ?1)",
        params![source],
    )?;
    conn.execute("DELETE FROM edges WHERE source_id = ?1", params![source])?;
    conn.execute("DELETE FROM nodes WHERE source_id = ?1", params![source])?;
    conn.execute("DELETE FROM sources WHERE id = ?1", params![source])?;
    Ok(())
}

fn delete_dangling_sql(conn: &Connection) -> Result<(), KnowledgeError> {
    conn.execute(
        "DELETE FROM edges WHERE from_id NOT IN (SELECT id FROM nodes) OR to_id NOT IN (SELECT id FROM nodes)",
        [],
    )?;
    Ok(())
}

fn insert_draft_sql(conn: &Connection, draft: &Draft) -> Result<(), KnowledgeError> {
    conn.execute(
        "INSERT INTO sources (id, fingerprint, language, byte_len) VALUES (?1, ?2, ?3, ?4)",
        params![
            draft.source_id,
            draft.fingerprint,
            draft.language,
            draft.byte_len
        ],
    )?;
    for node in &draft.nodes {
        insert_node_sql(conn, node)?;
    }
    for edge in &draft.edges {
        insert_edge_sql(conn, edge)?;
    }
    for frag in &draft.fragments {
        insert_fragment_sql(conn, frag)?;
    }
    Ok(())
}

fn insert_node_sql(conn: &Connection, node: &KnowledgeNode) -> Result<(), KnowledgeError> {
    insert_node_into(conn, "nodes", node)
}

fn insert_node_into(
    conn: &Connection,
    table: &str,
    node: &KnowledgeNode,
) -> Result<(), KnowledgeError> {
    let sql = format!(
        "INSERT INTO {table} (
            id, node_type, name, scope, summary, status,
            provenance_kind, provenance_source, provenance_location,
            extractor, extractor_version, method, confidence_millis, version,
            fingerprint, source_refs, retrieval_metadata, payload, source_id, parent
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)"
    );
    conn.execute(
        &sql,
        params![
            node.id,
            node.node_type.as_str(),
            node.name,
            node.scope,
            node.summary,
            node.status,
            node.provenance.kind.as_str(),
            node.provenance.source,
            node.provenance.location,
            node.provenance.extractor,
            node.provenance.extractor_version,
            node.provenance.method,
            node.confidence_millis,
            node.version,
            node.fingerprint,
            serde_json::to_string(&node.source_refs).unwrap_or_else(|_| "[]".into()),
            serde_json::to_string(&node.retrieval_metadata).unwrap_or_else(|_| "{}".into()),
            serde_json::to_string(&node.payload).unwrap_or_else(|_| "{}".into()),
            node.source_id,
            node.parent,
        ],
    )?;
    Ok(())
}

fn insert_edge_sql(conn: &Connection, edge: &KnowledgeEdge) -> Result<(), KnowledgeError> {
    insert_edge_into(conn, "edges", edge)
}

fn insert_edge_into(
    conn: &Connection,
    table: &str,
    edge: &KnowledgeEdge,
) -> Result<(), KnowledgeError> {
    let sql = format!(
        "INSERT INTO {table} (
            id, from_id, relation_type, to_id, confidence_millis,
            provenance_kind, provenance_source, provenance_location,
            extractor, extractor_version, method, metadata, source_id
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)"
    );
    conn.execute(
        &sql,
        params![
            edge.id,
            edge.from_id,
            edge.relation.as_str(),
            edge.to_id,
            edge.confidence_millis,
            edge.provenance.kind.as_str(),
            edge.provenance.source,
            edge.provenance.location,
            edge.provenance.extractor,
            edge.provenance.extractor_version,
            edge.provenance.method,
            serde_json::to_string(&edge.metadata).unwrap_or_else(|_| "{}".into()),
            edge.source_id,
        ],
    )?;
    Ok(())
}

fn insert_fragment_sql(conn: &Connection, frag: &KnowledgeFragment) -> Result<(), KnowledgeError> {
    conn.execute(
        "INSERT INTO fragments (
            id, node_id, source_ref, location, content, fingerprint, token_estimate, kind
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            frag.id,
            frag.node_id,
            frag.source_ref,
            frag.location,
            frag.content,
            frag.fingerprint,
            frag.token_estimate,
            frag.kind,
        ],
    )?;
    Ok(())
}

fn migrate(conn: &Connection) -> Result<(), KnowledgeError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS knowledge_migrations (
            version INTEGER PRIMARY KEY NOT NULL,
            name TEXT NOT NULL
        ) STRICT;",
    )?;
    let current: u32 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM knowledge_migrations",
        [],
        |row| row.get(0),
    )?;
    if current < 1 {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(MIGRATION_SQL)?;
        tx.execute(
            "INSERT INTO knowledge_migrations (version, name) VALUES (1, 'knowledge')",
            [],
        )?;
        tx.commit()?;
    }
    let current: u32 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM knowledge_migrations",
        [],
        |row| row.get(0),
    )?;
    if current < 2 {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(MIGRATION_V2_SQL)?;
        tx.execute(
            "INSERT INTO knowledge_migrations (version, name) VALUES (2, 'domain')",
            [],
        )?;
        tx.commit()?;
    }
    Ok(())
}

fn count(conn: &Connection, sql: &str) -> Result<usize, KnowledgeError> {
    let n: i64 = conn.query_row(sql, [], |row| row.get(0))?;
    Ok(usize::try_from(n).unwrap_or(0))
}

fn grouped(conn: &Connection, sql: &str) -> Result<BTreeMap<String, usize>, KnowledgeError> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut out = BTreeMap::new();
    for row in rows {
        let (key, n) = row?;
        out.insert(key, usize::try_from(n).unwrap_or(0));
    }
    Ok(out)
}

fn provenance_counts(conn: &Connection) -> Result<BTreeMap<String, usize>, KnowledgeError> {
    let mut out = grouped_merge(
        conn,
        "SELECT provenance_kind, COUNT(*) FROM nodes GROUP BY provenance_kind",
        "SELECT provenance_kind, COUNT(*) FROM domain_nodes GROUP BY provenance_kind",
    )?;
    let edges = grouped_merge(
        conn,
        "SELECT provenance_kind, COUNT(*) FROM edges GROUP BY provenance_kind",
        "SELECT provenance_kind, COUNT(*) FROM domain_edges GROUP BY provenance_kind",
    )?;
    for (kind, n) in edges {
        *out.entry(kind).or_default() += n;
    }
    Ok(out)
}

fn grouped_merge(
    conn: &Connection,
    left: &str,
    right: &str,
) -> Result<BTreeMap<String, usize>, KnowledgeError> {
    let mut out = grouped(conn, left)?;
    for (key, n) in grouped(conn, right)? {
        *out.entry(key).or_default() += n;
    }
    Ok(out)
}

fn node_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<KnowledgeNode> {
    let kind: String = row.get("node_type")?;
    let prov: String = row.get("provenance_kind")?;
    Ok(KnowledgeNode {
        id: row.get("id")?,
        node_type: NodeKind::parse(&kind).unwrap_or(NodeKind::Symbol),
        name: row.get("name")?,
        scope: row.get("scope")?,
        summary: row.get("summary")?,
        status: row.get("status")?,
        provenance: Provenance {
            kind: ProvenanceKind::parse(&prov).unwrap_or(ProvenanceKind::Observed),
            source: row.get("provenance_source")?,
            location: row.get("provenance_location")?,
            extractor: row.get("extractor")?,
            extractor_version: row.get("extractor_version")?,
            method: row.get("method")?,
        },
        confidence_millis: row.get("confidence_millis")?,
        version: row.get("version")?,
        fingerprint: row.get("fingerprint")?,
        source_refs: serde_json::from_str(&row.get::<_, String>("source_refs")?)
            .unwrap_or_default(),
        retrieval_metadata: serde_json::from_str(&row.get::<_, String>("retrieval_metadata")?)
            .unwrap_or_default(),
        payload: serde_json::from_str(&row.get::<_, String>("payload")?).unwrap_or_default(),
        source_id: row.get("source_id")?,
        parent: row.get("parent")?,
    })
}

fn edge_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<KnowledgeEdge> {
    let relation: String = row.get("relation_type")?;
    let prov: String = row.get("provenance_kind")?;
    Ok(KnowledgeEdge {
        id: row.get("id")?,
        from_id: row.get("from_id")?,
        relation: RelationType::parse(&relation).unwrap_or(RelationType::References),
        to_id: row.get("to_id")?,
        confidence_millis: row.get("confidence_millis")?,
        provenance: Provenance {
            kind: ProvenanceKind::parse(&prov).unwrap_or(ProvenanceKind::Observed),
            source: row.get("provenance_source")?,
            location: row.get("provenance_location")?,
            extractor: row.get("extractor")?,
            extractor_version: row.get("extractor_version")?,
            method: row.get("method")?,
        },
        metadata: serde_json::from_str(&row.get::<_, String>("metadata")?).unwrap_or_default(),
        source_id: row.get("source_id")?,
    })
}

fn table_nodes(conn: &Connection, table: &str) -> Result<Vec<KnowledgeNode>, KnowledgeError> {
    let sql = format!("SELECT * FROM {table} ORDER BY id");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], node_from_row)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(KnowledgeError::from)
}

fn table_edges(conn: &Connection, table: &str) -> Result<Vec<KnowledgeEdge>, KnowledgeError> {
    let sql = format!("SELECT * FROM {table} ORDER BY id");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], edge_from_row)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(KnowledgeError::from)
}

fn finding_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Finding> {
    let prov: String = row.get("provenance_kind")?;
    Ok(Finding {
        id: row.get("id")?,
        kind: row.get("kind")?,
        summary: row.get("summary")?,
        provenance: Provenance {
            kind: ProvenanceKind::parse(&prov).unwrap_or(ProvenanceKind::Inferred),
            source: row.get("provenance_source")?,
            location: row.get("provenance_location")?,
            extractor: row.get("extractor")?,
            extractor_version: row.get("extractor_version")?,
            method: row.get("method")?,
        },
        confidence_millis: row.get("confidence_millis")?,
        evidence: row.get("evidence")?,
        subjects: serde_json::from_str(&row.get::<_, String>("subjects")?).unwrap_or_default(),
    })
}

fn transition_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TransitionRecord> {
    Ok(TransitionRecord {
        id: row.get("id")?,
        node_id: row.get("node_id")?,
        from_status: row.get("from_status")?,
        to_status: row.get("to_status")?,
        actor: row.get("actor")?,
        reason: row.get("reason")?,
        occurred_at: row.get("occurred_at")?,
    })
}

fn insert_finding(conn: &Connection, finding: &Finding) -> Result<(), KnowledgeError> {
    conn.execute(
        "INSERT INTO domain_findings (
            id, kind, summary, provenance_kind, provenance_source, provenance_location,
            extractor, extractor_version, method, confidence_millis, evidence, subjects
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            finding.id,
            finding.kind,
            finding.summary,
            finding.provenance.kind.as_str(),
            finding.provenance.source,
            finding.provenance.location,
            finding.provenance.extractor,
            finding.provenance.extractor_version,
            finding.provenance.method,
            finding.confidence_millis,
            finding.evidence,
            serde_json::to_string(&finding.subjects).unwrap_or_else(|_| "[]".into()),
        ],
    )?;
    Ok(())
}

fn insert_transition(
    conn: &Connection,
    transition: &TransitionRecord,
) -> Result<(), KnowledgeError> {
    conn.execute(
        "INSERT INTO domain_transitions (
            id, node_id, from_status, to_status, actor, reason, occurred_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            transition.id,
            transition.node_id,
            transition.from_status,
            transition.to_status,
            transition.actor,
            transition.reason,
            transition.occurred_at,
        ],
    )?;
    Ok(())
}

fn fragment_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<KnowledgeFragment> {
    Ok(KnowledgeFragment {
        id: row.get("id")?,
        node_id: row.get("node_id")?,
        source_ref: row.get("source_ref")?,
        location: row.get("location")?,
        content: row.get("content")?,
        fingerprint: row.get("fingerprint")?,
        token_estimate: row.get("token_estimate")?,
        kind: row.get("kind")?,
    })
}

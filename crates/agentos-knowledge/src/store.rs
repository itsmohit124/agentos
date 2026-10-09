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
    KnowledgeEdge, KnowledgeFragment, KnowledgeNode, NodeKind, Provenance, ProvenanceKind,
    RelationType,
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

    /// Deletes every derived row. The schema stays.
    pub fn clear(&self) -> Result<(), KnowledgeError> {
        self.conn().execute_batch(
            "DELETE FROM fragments_fts; DELETE FROM fragments; DELETE FROM edges; DELETE FROM nodes; DELETE FROM sources;",
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

    /// All nodes, sorted by id.
    pub fn nodes(&self) -> Result<Vec<KnowledgeNode>, KnowledgeError> {
        let mut stmt = self.conn().prepare("SELECT * FROM nodes ORDER BY id")?;
        let rows = stmt.query_map([], node_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(KnowledgeError::from)
    }

    /// All edges, sorted by id.
    pub fn edges(&self) -> Result<Vec<KnowledgeEdge>, KnowledgeError> {
        self.edges_where("1 = 1")
    }

    /// Aggregated counts.
    pub fn stats(&self) -> Result<Stats, KnowledgeError> {
        let files = count(self.conn(), "SELECT COUNT(*) FROM sources")?;
        let nodes = count(self.conn(), "SELECT COUNT(*) FROM nodes")?;
        let edges = count(self.conn(), "SELECT COUNT(*) FROM edges")?;
        let fragments = count(self.conn(), "SELECT COUNT(*) FROM fragments")?;
        Ok(Stats {
            files,
            nodes,
            edges,
            fragments,
            nodes_by_type: grouped(
                self.conn(),
                "SELECT node_type, COUNT(*) FROM nodes GROUP BY node_type",
            )?,
            edges_by_type: grouped(
                self.conn(),
                "SELECT relation_type, COUNT(*) FROM edges GROUP BY relation_type",
            )?,
            provenance: provenance_counts(self.conn())?,
            schema_version: self.schema_version()?,
        })
    }

    /// One node, its edges, and its fragments.
    pub fn inspect(&self, id: &str) -> Result<Inspection, KnowledgeError> {
        let node = self
            .conn()
            .query_row("SELECT * FROM nodes WHERE id = ?1", params![id], |row| {
                node_from_row(row)
            })
            .optional()?
            .ok_or_else(|| KnowledgeError::NotFound(id.to_owned()))?;
        Ok(Inspection {
            outgoing: self.edges_where_param("from_id = ?1", id)?,
            incoming: self.edges_where_param("to_id = ?1", id)?,
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
        clause: &str,
        id: &str,
    ) -> Result<Vec<KnowledgeEdge>, KnowledgeError> {
        let sql = format!("SELECT * FROM edges WHERE {clause} ORDER BY id");
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
    conn.execute(
        "INSERT INTO nodes (
            id, node_type, name, scope, summary, status,
            provenance_kind, provenance_source, provenance_location,
            extractor, extractor_version, method, confidence_millis, version,
            fingerprint, source_refs, retrieval_metadata, payload, source_id, parent
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
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
    conn.execute(
        "INSERT INTO edges (
            id, from_id, relation_type, to_id, confidence_millis,
            provenance_kind, provenance_source, provenance_location,
            extractor, extractor_version, method, metadata, source_id
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
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
    if current >= 1 {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(MIGRATION_SQL)?;
    tx.execute(
        "INSERT INTO knowledge_migrations (version, name) VALUES (1, 'knowledge')",
        [],
    )?;
    tx.commit()?;
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
    let mut out = grouped(
        conn,
        "SELECT provenance_kind, COUNT(*) FROM nodes GROUP BY provenance_kind",
    )?;
    let edges = grouped(
        conn,
        "SELECT provenance_kind, COUNT(*) FROM edges GROUP BY provenance_kind",
    )?;
    for (kind, n) in edges {
        *out.entry(kind).or_default() += n;
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

-- Stored knowledge schema version 1, from before the domain tables.
-- Opening this file with KnowledgeStore must apply version 2 and leave version 1 tables in place.
CREATE TABLE knowledge_migrations (
    version INTEGER PRIMARY KEY NOT NULL,
    name TEXT NOT NULL
) STRICT;
INSERT INTO knowledge_migrations (version, name) VALUES (1, 'knowledge');
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

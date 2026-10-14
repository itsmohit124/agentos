//! Upgrade of a stored knowledge schema version 1 database.

use std::fs;
use std::path::Path;

use agentos_knowledge::{KnowledgeStore, rebuild, scan};
use rusqlite::Connection;

#[test]
fn stored_v1_database_upgrades_to_domain_tables() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let state = root.join(".agentos");
    fs::create_dir_all(&state).unwrap();
    let db_path = state.join("agentos.db");
    let sql = include_str!("fixtures/knowledge-v1.sql");
    Connection::open(&db_path)
        .unwrap()
        .execute_batch(sql)
        .unwrap();

    let store = KnowledgeStore::open(root).unwrap();
    assert_eq!(store.schema_version().unwrap(), 2);
    drop(store);

    let conn = Connection::open(&db_path).unwrap();
    assert!(table_exists(&conn, "nodes"));
    assert!(table_exists(&conn, "domain_nodes"));
    assert!(table_exists(&conn, "domain_edges"));
    assert!(table_exists(&conn, "domain_transitions"));
    assert!(table_exists(&conn, "domain_findings"));
    let names = migration_names(&conn);
    assert_eq!(names, vec!["domain".to_owned(), "knowledge".to_owned()]);
    drop(conn);

    let again = KnowledgeStore::open(root).unwrap();
    assert_eq!(again.schema_version().unwrap(), 2);
}

#[test]
fn rebuild_replaces_a_corrupt_index() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("app")).unwrap();
    fs::write(root.join("app/pay.py"), "class Payment:\n    pass\n").unwrap();
    scan(root).unwrap();
    let before = KnowledgeStore::open(root).unwrap().fingerprints().unwrap();
    drop_store_and_corrupt(&root.join(".agentos/agentos.db"));
    rebuild(root).unwrap();
    let after = KnowledgeStore::open(root).unwrap().fingerprints().unwrap();
    assert_eq!(after, before);
    assert!(after.contains_key("app/pay.py"));
}

fn drop_store_and_corrupt(path: &Path) {
    fs::write(path, b"this is not a sqlite database").unwrap();
}

fn table_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE name = ?1)",
        [name],
        |row| row.get(0),
    )
    .unwrap()
}

fn migration_names(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT name FROM knowledge_migrations ORDER BY name")
        .unwrap();
    stmt.query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<String>, _>>()
        .unwrap()
}

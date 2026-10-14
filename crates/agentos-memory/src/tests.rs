//! Store, trust, staleness, and redaction tests.

use std::fs;
use std::path::Path;

use agentos_core::db::Database;
use agentos_core::state::{DB_FILE, STATE_DIR};
use agentos_knowledge::{KnowledgeStore, ProvenanceKind};

use crate::{FingerprintIndex, MemoryLink, MemoryStore, NewMemory, RunCapture, record_run};

fn draft(content: &str) -> NewMemory {
    NewMemory {
        memory_type: "lesson".to_owned(),
        name: "Idempotency".to_owned(),
        content: content.to_owned(),
        task: "Add retry support to payment webhook processing".to_owned(),
        run_id: "run-1".to_owned(),
        outcome: "failed".to_owned(),
        links: vec![MemoryLink {
            kind: "source".to_owned(),
            id: "app/payments/service.py".to_owned(),
            fingerprint: "fp-1".to_owned(),
        }],
        recorded_at: "1970-01-01T00:00:00Z".to_owned(),
        confirm: false,
        pattern: false,
        intent_kind: String::new(),
        payload: Default::default(),
        provenance_kind: ProvenanceKind::Inferred,
        method: "failure-lesson".to_owned(),
        confidence_millis: 400,
    }
}

fn open(dir: &Path) -> MemoryStore {
    MemoryStore::open(dir).unwrap()
}

#[test]
fn new_memory_is_proposed_and_confirm_never_writes_canonical() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let created = store.add(draft("missing idempotency")).unwrap();
    assert_eq!(created.status, "proposed");
    assert!(!created.is_canonical());
    assert_eq!(created.provenance.kind, ProvenanceKind::Inferred);
    let confirmed = store
        .confirm(&created.id, "operator", "1970-01-01T00:00:01Z")
        .unwrap();
    assert_eq!(confirmed.status, "confirmed");
    assert!(!confirmed.is_canonical());
    assert_ne!(confirmed.status, "canonical");
}

#[test]
fn a_canonical_file_is_rewritten_as_proposed() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let created = store.add(draft("missing idempotency")).unwrap();
    drop(store);
    let path = dir
        .path()
        .join(".agentos/memory")
        .join(format!("{}.json", created.id));
    let mut value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    value["status"] = serde_json::json!("canonical");
    fs::write(&path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
    let store = open(dir.path());
    let loaded = store.get(&created.id).unwrap().unwrap();
    assert_eq!(loaded.status, "proposed");
    assert!(!loaded.is_canonical());
    let text = fs::read_to_string(&path).unwrap();
    assert!(!text.contains("canonical"));
}

#[test]
fn secrets_are_redacted_before_the_file_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let created = store
        .add(draft("password=hunter2 AKIAIOSFODNN7EXAMPLE"))
        .unwrap();
    let path = dir
        .path()
        .join(".agentos/memory")
        .join(format!("{}.json", created.id));
    let text = fs::read_to_string(path).unwrap();
    assert!(!text.contains("hunter2"));
    assert!(!text.contains("AKIAIOSFODNN7EXAMPLE"));
    assert!(text.contains("[REDACTED]"));
}

#[test]
fn forgotten_memory_is_not_a_candidate_and_is_not_restored() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let created = store.add(draft("missing idempotency")).unwrap();
    store.forget(&created.id, "1970-01-01T00:00:02Z").unwrap();
    let again = store.add(draft("missing idempotency")).unwrap();
    assert_eq!(again.status, "forgotten");
    let index = FingerprintIndex::new();
    let candidates = store.candidates(&index, "1970-01-01T00:00:03Z").unwrap();
    assert!(candidates.iter().all(|item| item.id != created.id));
}

#[test]
fn changed_fingerprint_marks_stale_and_prune_removes_only_stale() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let created = store.add(draft("missing idempotency")).unwrap();
    let mut kept = draft("semantic fact about payments");
    kept.memory_type = "semantic".to_owned();
    kept.outcome.clear();
    kept.links.clear();
    kept.method = "human-add".to_owned();
    kept.provenance_kind = ProvenanceKind::Declared;
    let other = store.add(kept).unwrap();
    let mut index = FingerprintIndex::new();
    index.insert("app/payments/service.py", "fp-2");
    let removed = store.prune_stale(&index, "1970-01-01T00:00:04Z").unwrap();
    assert_eq!(removed, vec![created.id.clone()]);
    assert!(store.get(&created.id).unwrap().is_none());
    assert_eq!(store.get(&other.id).unwrap().unwrap().status, "proposed");
}

#[test]
fn matching_fingerprint_is_fresh_and_a_change_is_stale() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let created = store.add(draft("missing idempotency")).unwrap();
    let mut index = FingerprintIndex::new();
    index.insert("app/payments/service.py", "fp-1");
    let shown = store
        .show(&created.id, &index, "1970-01-01T00:00:05Z")
        .unwrap();
    assert_eq!(shown.freshness, "fresh");
    index.insert("app/payments/service.py", "fp-2");
    let stale = store
        .show(&created.id, &index, "1970-01-01T00:00:06Z")
        .unwrap();
    assert_eq!(stale.freshness, "stale");
    assert_eq!(stale.status, "proposed");
    index.insert("app/payments/service.py", "fp-1");
    let healed = store
        .show(&created.id, &index, "1970-01-01T00:00:07Z")
        .unwrap();
    assert_eq!(healed.freshness, "fresh");
}

#[test]
fn a_failed_run_records_a_proposed_lesson() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let records = record_run(
        &store,
        &RunCapture {
            run_id: "run-9".to_owned(),
            task: "Add retry support to payment webhook processing".to_owned(),
            outcome: "failed".to_owned(),
            context_pack_id: "pack-1".to_owned(),
            pack_ids: vec!["pack-1".to_owned()],
            agent_summary: "webhook handler returned 500".to_owned(),
            validation: "unit_test failed".to_owned(),
            failures: vec!["missing idempotency".to_owned()],
            links: vec![MemoryLink {
                kind: "source".to_owned(),
                id: "app/payments/service.py".to_owned(),
                fingerprint: "fp-1".to_owned(),
            }],
            recorded_at: "1970-01-01T00:00:00Z".to_owned(),
            intent_kind: "feature".to_owned(),
            shape: vec!["class".to_owned()],
        },
    )
    .unwrap();
    let lesson = records
        .iter()
        .find(|record| record.memory_type == "lesson")
        .unwrap();
    assert_eq!(lesson.status, "proposed");
    assert!(!lesson.is_canonical());
    assert!(lesson.content.contains("missing idempotency"));
    assert_eq!(lesson.provenance.method, "failure-lesson");
    assert!(
        records
            .iter()
            .any(|record| record.memory_type == "episodic")
    );
    assert!(
        records
            .iter()
            .all(|record| record.memory_type != "procedural")
    );
}

#[test]
fn knowledge_schema_stays_at_2_and_core_meta_stays_at_1() {
    let dir = tempfile::tempdir().unwrap();
    let _store = open(dir.path());
    let (db, _) = Database::open(&dir.path().join(STATE_DIR).join(DB_FILE)).unwrap();
    assert_eq!(db.schema_version().unwrap(), 1);
    let knowledge = KnowledgeStore::open(dir.path()).unwrap();
    assert_eq!(knowledge.schema_version().unwrap(), 2);
    let _reopened = open(dir.path());
    assert_eq!(knowledge.schema_version().unwrap(), 2);
}

#[test]
fn deleting_the_database_rebuilds_rows_from_files() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let created = store.add(draft("missing idempotency")).unwrap();
    drop(store);
    fs::remove_file(dir.path().join(STATE_DIR).join(DB_FILE)).unwrap();
    let store = open(dir.path());
    let loaded = store.get(&created.id).unwrap().unwrap();
    assert!(loaded.content.contains("idempotency"));
    assert_eq!(loaded.status, "proposed");
}

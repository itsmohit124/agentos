//! Graph, extractor, and scan tests.

use std::fs;
use std::path::Path;

use crate::extract::{ExtractorRegistry, LanguageExtractor};
use crate::languages::{PythonExtractor, TypeScriptExtractor};
use crate::model::{NodeKind, ProvenanceKind, RelationType};
use crate::scan::{rebuild, scan, scan_with};

const PAYMENT_SERVICE: &str = r#"
class PaymentService:
    """Charges payments through the repository."""

    def charge(self, repository: PaymentRepository, payment: Payment) -> Payment:
        """Charge a payment by saving it."""
        return repository.save(payment)
"#;

#[test]
fn python_extractor_records_annotated_calls() {
    let symbols = PythonExtractor
        .extract("app/payments/service.py", PAYMENT_SERVICE)
        .unwrap();
    let class = symbols.iter().find(|s| s.name == "PaymentService").unwrap();
    assert_eq!(class.kind, NodeKind::Class);
    let method = symbols.iter().find(|s| s.name == "charge").unwrap();
    assert_eq!(method.parent.as_deref(), Some("PaymentService"));
    assert_eq!(method.calls.len(), 1);
    assert_eq!(method.calls[0].callee, "save");
    assert_eq!(method.calls[0].receiver.as_deref(), Some("repository"));
    assert_eq!(
        method.calls[0].receiver_type.as_deref(),
        Some("PaymentRepository")
    );
}

#[test]
fn typescript_extractor_records_implements_and_calls() {
    let source = r#"
interface PaymentStore {
  save(id: string): void;
}
class PaymentService implements PaymentStore {
  save(id: string): void {}
  charge(store: PaymentStore): void {
    store.save("1");
  }
}
"#;
    let symbols = TypeScriptExtractor.extract("pay.ts", source).unwrap();
    assert!(
        symbols
            .iter()
            .any(|s| s.kind == NodeKind::Interface && s.name == "PaymentStore")
    );
    let class = symbols.iter().find(|s| s.name == "PaymentService").unwrap();
    assert_eq!(class.implements, vec!["PaymentStore".to_owned()]);
    let charge = symbols.iter().find(|s| s.name == "charge").unwrap();
    assert_eq!(charge.calls[0].callee, "save");
    assert_eq!(
        charge.calls[0].receiver_type.as_deref(),
        Some("PaymentStore")
    );
}

struct StubExtractor;

impl LanguageExtractor for StubExtractor {
    fn name(&self) -> &'static str {
        "stub"
    }
    fn version(&self) -> &'static str {
        "1"
    }
    fn matches(&self, path: &Path) -> bool {
        path.extension().and_then(|e| e.to_str()) == Some("stub")
    }
    fn extract(
        &self,
        _relative: &str,
        source: &str,
    ) -> Result<Vec<crate::RawSymbol>, crate::KnowledgeError> {
        let mut symbols = PythonExtractor.extract("x.py", "def stubbed():\n    return 1\n")?;
        symbols[0].summary = source.trim().to_owned();
        Ok(symbols)
    }
}

fn touch(dir: &Path, rel: &str, body: &str) {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
}

#[test]
fn scan_links_calls_tests_and_mentions_without_promoting_inferred() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    touch(
        root,
        "app/payments/models.py",
        "class Payment:\n    \"\"\"A payment.\"\"\"\n    pass\n",
    );
    touch(
        root,
        "app/payments/repository.py",
        "class BaseRepository:\n    def save(self, payment: Payment) -> Payment:\n        return payment\n\nclass PaymentRepository(BaseRepository):\n    def save(self, payment: Payment) -> Payment:\n        return payment\n",
    );
    touch(root, "app/payments/service.py", PAYMENT_SERVICE);
    touch(
        root,
        "app/payments/controller.py",
        "class PaymentController:\n    def create(self, service: PaymentService, payment: Payment) -> Payment:\n        return service.charge(payment)\n",
    );
    touch(
        root,
        "tests/test_payments.py",
        "def test_charge_saves_payment() -> None:\n    payment = Payment(\"pay-1\", 100)\n    repository = PaymentRepository()\n    service = PaymentService()\n    service.charge(repository, payment)\n",
    );
    touch(
        root,
        "docs/overview.md",
        "# Payments\n\nA Payment through PaymentService.\n",
    );
    touch(root, ".agentosignore", "expected/\n");

    let report = scan(root).unwrap();
    assert!(report.rows_written > 0, "{report:?}");
    let store = crate::KnowledgeStore::open(root).unwrap();
    assert!(store.provenance_complete().unwrap());
    let edges = store.edges().unwrap();
    let has = |from: &str, rel: RelationType, to: &str| {
        edges
            .iter()
            .any(|e| e.from_id == from && e.relation == rel && e.to_id == to)
    };
    assert!(has(
        "software.app.payments.controller.class.PaymentController",
        RelationType::Calls,
        "software.app.payments.service.class.PaymentService"
    ));
    assert!(has(
        "software.app.payments.service.class.PaymentService",
        RelationType::Calls,
        "software.app.payments.repository.class.PaymentRepository"
    ));
    assert!(has(
        "software.app.payments.repository.class.PaymentRepository",
        RelationType::Extends,
        "software.app.payments.repository.class.BaseRepository"
    ));
    assert!(has(
        "quality.tests.test_payments.test.test_charge_saves_payment",
        RelationType::Tests,
        "software.app.payments.service.class.PaymentService"
    ));
    let mention = edges
        .iter()
        .find(|e| e.relation == RelationType::References)
        .unwrap();
    assert_eq!(mention.provenance.kind, ProvenanceKind::Inferred);
    assert_eq!(mention.provenance.method, "name-mention");

    let second = scan(root).unwrap();
    assert_eq!(second.changed, 0, "{second:?}");
    assert_eq!(second.rows_written, 0, "{second:?}");

    let before = store.nodes().unwrap();
    touch(
        root,
        "app/payments/service.py",
        &PAYMENT_SERVICE.replace("Charges payments", "Charges a payment"),
    );
    let third = scan(root).unwrap();
    assert_eq!(third.changed, 1, "{third:?}");
    let after = crate::KnowledgeStore::open(root).unwrap().nodes().unwrap();
    let changed_sources: Vec<_> = after
        .iter()
        .filter(|node| {
            before
                .iter()
                .any(|old| old.id == node.id && old.fingerprint != node.fingerprint)
        })
        .map(|node| node.source_id.clone())
        .collect();
    assert!(
        changed_sources
            .iter()
            .all(|s| s == "app/payments/service.py"),
        "{changed_sources:?}"
    );
    let untouched = after
        .iter()
        .find(|n| n.id.ends_with("class.PaymentController"))
        .unwrap();
    let old = before.iter().find(|n| n.id == untouched.id).unwrap();
    assert_eq!(untouched.fingerprint, old.fingerprint);
}

#[test]
fn moving_between_source_roots_keeps_the_logical_id() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let body = "class Payment:\n    \"\"\"A payment.\"\"\"\n    pass\n";
    touch(root, "src/payments/models.py", body);
    scan(root).unwrap();
    let store = crate::KnowledgeStore::open(root).unwrap();
    let id = "software.payments.models.class.Payment";
    assert!(store.inspect(id).is_ok());
    fs::remove_file(root.join("src/payments/models.py")).unwrap();
    touch(root, "lib/payments/models.py", body);
    scan(root).unwrap();
    let store = crate::KnowledgeStore::open(root).unwrap();
    let inspection = store.inspect(id).unwrap();
    assert_eq!(inspection.node.source_id, "lib/payments/models.py");
    assert!(
        store
            .nodes()
            .unwrap()
            .iter()
            .all(|n| n.source_id != "src/payments/models.py")
    );
}

#[test]
fn rebuild_and_deleted_database_reproduce_the_graph() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    touch(root, "app/payments/service.py", PAYMENT_SERVICE);
    scan(root).unwrap();
    let snapshot = |root: &Path| {
        let store = crate::KnowledgeStore::open(root).unwrap();
        let mut nodes: Vec<_> = store
            .nodes()
            .unwrap()
            .into_iter()
            .map(|n| (n.id, n.fingerprint))
            .collect();
        nodes.sort();
        let mut edges: Vec<_> = store.edges().unwrap().into_iter().map(|e| e.id).collect();
        edges.sort();
        (nodes, edges)
    };
    let first = snapshot(root);
    rebuild(root).unwrap();
    assert_eq!(first, snapshot(root));
    fs::remove_file(root.join(".agentos/agentos.db")).unwrap();
    scan(root).unwrap();
    assert_eq!(first, snapshot(root));
}

#[test]
fn fts_finds_fragment_text_and_a_stub_extractor_plugs_in() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    touch(root, "notes.stub", "hello from stub\n");
    let mut registry = ExtractorRegistry::new();
    registry.register(Box::new(StubExtractor));
    scan_with(root, &registry).unwrap();
    let store = crate::KnowledgeStore::open(root).unwrap();
    let nodes = store.nodes().unwrap();
    assert!(nodes.iter().any(|n| n.provenance.extractor == "stub"));
    let hits = store.search("stubbed").unwrap();
    assert!(!hits.is_empty(), "fts missed the stub function");
}

#[test]
fn typescript_scan_links_implements() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    touch(
        root,
        "src/pay.ts",
        "interface PaymentStore {\n  save(id: string): void;\n}\nclass PaymentService implements PaymentStore {\n  save(id: string): void {}\n}\n",
    );
    scan(root).unwrap();
    let store = crate::KnowledgeStore::open(root).unwrap();
    let edges = store.edges().unwrap();
    assert!(
        edges.iter().any(|e| e.relation == RelationType::Implements),
        "{edges:?}"
    );
    assert!(store.provenance_complete().unwrap());
}

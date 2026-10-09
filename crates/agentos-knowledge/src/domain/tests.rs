//! Contract tests for domain lifecycle, heuristics, and generated markdown.

use std::fs;
use std::path::{Path, PathBuf};

use crate::model::{KnowledgeNode, NodeKind, ProvenanceKind, RelationType};
use crate::store::KnowledgeStore;
use crate::{Lifecycle, confirm_with, domain_scan_with, propose_with, reject_with, validate};

use super::{DomainProposal, DomainProposer, FixedClock};

fn clock() -> FixedClock {
    FixedClock {
        instant: "CLOCK-SHOULD-NOT-APPEAR".to_owned(),
    }
}

fn touch(dir: &Path, rel: &str, body: &str) {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
}

fn payments_project(root: &Path) {
    touch(
        root,
        "app/payments/models.py",
        "\
class Money:
    \"\"\"A monetary amount. Value object.\"\"\"
    def __init__(self, amount: int, currency: str) -> None:
        self.amount = amount
        self.currency = currency

class Payment:
    \"\"\"A payment to capture.\"\"\"
    def __init__(self, payment_id: str, amount: int) -> None:
        if amount <= 0:
            raise ValueError(\"amount > 0\")
        self.payment_id = payment_id
        self.amount = amount
        self.money = Money(amount, \"USD\")

class PaymentAggregate:
    \"\"\"Aggregate root for a payment and its money.\"\"\"
    def __init__(self, payment: Payment) -> None:
        self.root = payment
",
    );
    touch(
        root,
        "app/payments/service.py",
        "class PaymentService:\n    \"\"\"Charges payments through the repository.\"\"\"\n    def charge(self, repository: PaymentRepository, payment: Payment) -> Payment:\n        return repository.save(payment)\n",
    );
    touch(
        root,
        "app/payments/repository.py",
        "class BaseRepository:\n    def save(self, payment: Payment) -> Payment:\n        return payment\n\nclass PaymentRepository(BaseRepository):\n    def save(self, payment: Payment) -> Payment:\n        return payment\n",
    );
    touch(
        root,
        "app/users/models.py",
        "class User:\n    \"\"\"A user account.\"\"\"\n    def __init__(self, user_id: str) -> None:\n        self.user_id = user_id\n",
    );
    touch(
        root,
        "app/users/service.py",
        "class UserService:\n    def register(self, notifications: NotificationService, user: User) -> str:\n        return notifications.send(user.user_id, \"welcome\")\n",
    );
    touch(
        root,
        "app/notifications/service.py",
        "class NotificationService:\n    def send(self, user_id: str, message: str) -> str:\n        return message\n",
    );
    touch(
        root,
        "tests/test_payments.py",
        "def test_amount_must_be_positive() -> None:\n    \"\"\"Invariant: amount > 0.\"\"\"\n    Payment(\"pay-0\", 0)\n",
    );
}

struct CanonProposer;

impl DomainProposer for CanonProposer {
    fn name(&self) -> &str {
        "bad"
    }

    fn propose(
        &self,
        _existing: &[KnowledgeNode],
    ) -> Result<Vec<DomainProposal>, crate::KnowledgeError> {
        let mut node = _existing
            .iter()
            .find(|node| node.node_type == NodeKind::Entity)
            .cloned()
            .expect("entity");
        node.id = "domain.payments.entity.Smuggled".to_owned();
        node.name = "Smuggled".to_owned();
        node.status = Lifecycle::Canonical.as_str().to_owned();
        Ok(vec![DomainProposal {
            node,
            edges: Vec::new(),
        }])
    }
}

#[test]
fn scan_and_propose_never_write_confirmed_or_canonical() {
    let tmp = tempfile::tempdir().unwrap();
    payments_project(tmp.path());
    let report = domain_scan_with(tmp.path(), &clock()).unwrap();
    assert!(report.nodes > 0, "{report:?}");
    let store = KnowledgeStore::open(tmp.path()).unwrap();
    let nodes = store.domain_nodes().unwrap();
    assert!(
        nodes
            .iter()
            .all(|node| { matches!(node.status.as_str(), "observed" | "inferred" | "proposed") }),
        "{:?}",
        nodes
            .iter()
            .map(|node| (&node.id, &node.status))
            .collect::<Vec<_>>()
    );
    assert!(nodes.iter().all(|node| {
        node.provenance.is_complete()
            && !node.provenance.source.is_empty()
            && !node.provenance.location.is_empty()
            && node.confidence_millis > 0
            && node.payload.contains_key("evidence")
    }));
    let proposed = propose_with(tmp.path(), &crate::NullProposer, &clock()).unwrap();
    assert!(proposed.promoted > 0);
    let store = KnowledgeStore::open(tmp.path()).unwrap();
    assert!(
        store
            .domain_nodes()
            .unwrap()
            .iter()
            .all(|node| { node.status != "confirmed" && node.status != "canonical" })
    );
    let err = propose_with(tmp.path(), &CanonProposer, &clock()).unwrap_err();
    assert!(
        err.to_string().contains("tried to write canonical"),
        "{err}"
    );
    assert!(
        KnowledgeStore::open(tmp.path())
            .unwrap()
            .domain_nodes()
            .unwrap()
            .iter()
            .all(|node| node.name != "Smuggled")
    );
}

#[test]
fn payment_links_money_and_the_amount_invariant() {
    let tmp = tempfile::tempdir().unwrap();
    payments_project(tmp.path());
    domain_scan_with(tmp.path(), &clock()).unwrap();
    let store = KnowledgeStore::open(tmp.path()).unwrap();
    let nodes = store.domain_nodes().unwrap();
    let edges = store.domain_edges().unwrap();
    let has = |id: &str| nodes.iter().any(|node| node.id == id);
    assert!(has("domain.payments.bounded_context.Payments"));
    assert!(has("domain.users.bounded_context.Users"));
    assert!(has("domain.notifications.bounded_context.Notifications"));
    assert!(has("domain.payments.entity.Payment"));
    assert!(has("domain.payments.value_object.Money"));
    assert!(has("domain.payments.aggregate.PaymentAggregate"));
    assert!(has("domain.payments.invariant.amount-gt-0"));
    assert!(edges.iter().any(|edge| {
        edge.from_id == "domain.payments.entity.Payment"
            && edge.relation == RelationType::HasValueObject
            && edge.to_id == "domain.payments.value_object.Money"
    }));
    assert!(edges.iter().any(|edge| {
        edge.from_id == "domain.payments.aggregate.PaymentAggregate"
            && edge.relation == RelationType::ConstrainedBy
            && edge.to_id == "domain.payments.invariant.amount-gt-0"
    }));
    assert!(edges.iter().any(|edge| {
        edge.relation == RelationType::CrossContextTo
            && edge.from_id == "domain.users.bounded_context.Users"
            && edge.to_id == "domain.notifications.bounded_context.Notifications"
    }));
    let second = domain_scan_with(tmp.path(), &clock()).unwrap();
    assert_eq!(second.rows_written, 0, "{second:?}");
}

#[test]
fn confirm_rejects_illegal_states_and_a_missing_id() {
    let tmp = tempfile::tempdir().unwrap();
    payments_project(tmp.path());
    domain_scan_with(tmp.path(), &clock()).unwrap();
    let missing = confirm_with(
        tmp.path(),
        "domain.payments.entity.Missing",
        "operator",
        "because",
        &clock(),
    );
    assert!(
        missing
            .unwrap_err()
            .to_string()
            .contains("no knowledge node")
    );
    let early = confirm_with(
        tmp.path(),
        "domain.payments.entity.Payment",
        "operator",
        "too soon",
        &clock(),
    );
    assert!(
        early
            .unwrap_err()
            .to_string()
            .contains("illegal domain transition")
    );
    propose_with(tmp.path(), &crate::NullProposer, &clock()).unwrap();
    let change = confirm_with(
        tmp.path(),
        "domain.payments.entity.Payment",
        "operator",
        "operator confirmation",
        &clock(),
    )
    .unwrap();
    assert_eq!(change.from, "proposed");
    assert_eq!(change.to, "confirmed");
    let again = confirm_with(
        tmp.path(),
        "domain.payments.entity.Payment",
        "operator",
        "operator canonical promotion",
        &clock(),
    )
    .unwrap();
    assert_eq!(again.to, "canonical");
    let rejected = reject_with(
        tmp.path(),
        "domain.payments.entity.Payment",
        "operator",
        "too late",
        &clock(),
    );
    assert!(
        rejected
            .unwrap_err()
            .to_string()
            .contains("illegal domain transition")
    );
    let store = KnowledgeStore::open(tmp.path()).unwrap();
    let records = store
        .transitions_for("domain.payments.entity.Payment")
        .unwrap();
    assert!(records.iter().any(|record| {
        record.actor == "operator"
            && record.reason == "operator confirmation"
            && record.to_status == "confirmed"
            && record.occurred_at == "CLOCK-SHOULD-NOT-APPEAR"
    }));
    let markdown =
        fs::read_to_string(tmp.path().join(".agentos/knowledge/domain/aggregates.md")).unwrap();
    assert!(!markdown.contains("CLOCK-SHOULD-NOT-APPEAR"));
    assert!(markdown.contains("## Confirmed and canonical"));
    assert!(markdown.contains("## Proposed"));
}

#[test]
fn validate_reports_drift_when_canonical_evidence_is_deleted() {
    let tmp = tempfile::tempdir().unwrap();
    payments_project(tmp.path());
    domain_scan_with(tmp.path(), &clock()).unwrap();
    propose_with(tmp.path(), &crate::NullProposer, &clock()).unwrap();
    confirm_with(
        tmp.path(),
        "domain.payments.entity.Payment",
        "operator",
        "yes",
        &clock(),
    )
    .unwrap();
    confirm_with(
        tmp.path(),
        "domain.payments.entity.Payment",
        "operator",
        "canon",
        &clock(),
    )
    .unwrap();
    fs::remove_file(tmp.path().join("app/payments/models.py")).unwrap();
    let report = validate(tmp.path()).unwrap();
    assert!(!report.ok, "{report:?}");
    assert!(
        report.drift.iter().any(|line| line.contains("models.py")),
        "{report:?}"
    );
}

#[test]
fn rescan_keeps_confirmed_and_software_scan_keeps_domain_rows() {
    let tmp = tempfile::tempdir().unwrap();
    payments_project(tmp.path());
    domain_scan_with(tmp.path(), &clock()).unwrap();
    propose_with(tmp.path(), &crate::NullProposer, &clock()).unwrap();
    confirm_with(
        tmp.path(),
        "domain.payments.entity.Payment",
        "operator",
        "yes",
        &clock(),
    )
    .unwrap();
    domain_scan_with(tmp.path(), &clock()).unwrap();
    let store = KnowledgeStore::open(tmp.path()).unwrap();
    let payment = store
        .domain_nodes()
        .unwrap()
        .into_iter()
        .find(|node| node.id == "domain.payments.entity.Payment")
        .unwrap();
    assert_eq!(payment.status, "confirmed");
    crate::scan(tmp.path()).unwrap();
    let store = KnowledgeStore::open(tmp.path()).unwrap();
    assert!(
        store.domain_nodes().unwrap().iter().any(|node| {
            node.id == "domain.payments.entity.Payment" && node.status == "confirmed"
        })
    );
}

#[test]
fn ambiguity_is_a_finding_and_not_a_merge() {
    let tmp = tempfile::tempdir().unwrap();
    touch(
        tmp.path(),
        "app/billing/models.py",
        "\
class Payment:
    def __init__(self, payment_id: str) -> None:
        self.payment_id = payment_id

class Charge:
    def __init__(self, charge_id: str) -> None:
        self.charge_id = charge_id

class Transaction:
    def __init__(self, transaction_id: str) -> None:
        self.transaction_id = transaction_id
",
    );
    domain_scan_with(tmp.path(), &clock()).unwrap();
    let store = KnowledgeStore::open(tmp.path()).unwrap();
    let nodes = store.domain_nodes().unwrap();
    for name in ["Payment", "Charge", "Transaction"] {
        assert!(
            nodes
                .iter()
                .any(|node| node.node_type == NodeKind::Entity && node.name == name),
            "{name}"
        );
    }
    assert_eq!(
        nodes
            .iter()
            .filter(|node| node.node_type == NodeKind::Entity && node.scope == "billing")
            .count(),
        3
    );
    let findings = store.domain_findings().unwrap();
    assert!(
        findings
            .iter()
            .any(|finding| finding.kind == "terminology_ambiguity"),
        "{findings:?}"
    );
    assert!(nodes.iter().all(|node| node.status != "canonical"));
}

#[test]
fn hexagonal_leak_is_suppressed_by_an_adr_exception() {
    let body = "\
import sqlalchemy

class Order:
    \"\"\"An order.\"\"\"
    def __init__(self, order_id: str) -> None:
        self.order_id = order_id
";
    let leak = tempfile::tempdir().unwrap();
    touch(leak.path(), "app/orders/domain/model.py", body);
    domain_scan_with(leak.path(), &clock()).unwrap();
    let findings = KnowledgeStore::open(leak.path())
        .unwrap()
        .domain_findings()
        .unwrap();
    assert!(
        findings
            .iter()
            .any(|finding| finding.kind == "hexagonal_leak"),
        "{findings:?}"
    );

    let allowed = tempfile::tempdir().unwrap();
    touch(allowed.path(), "app/orders/domain/model.py", body);
    touch(
        allowed.path(),
        "docs/adr/0001-sqlalchemy-in-domain.md",
        "\
# ADR 0001: SQLAlchemy inside the order domain

## Status

Accepted

## Decision

sqlalchemy is an allowed exception inside the order domain.
",
    );
    domain_scan_with(allowed.path(), &clock()).unwrap();
    let findings = KnowledgeStore::open(allowed.path())
        .unwrap()
        .domain_findings()
        .unwrap();
    assert!(
        findings
            .iter()
            .all(|finding| finding.kind != "hexagonal_leak"),
        "{findings:?}"
    );
}

#[test]
fn greenfield_requirements_are_proposed_not_confirmed() {
    let tmp = tempfile::tempdir().unwrap();
    touch(
        tmp.path(),
        "docs/spec/requirements.md",
        "\
# Billing requirements

The billing area has no code yet.

Billing is a bounded context.

Invoice is an entity in Billing.

Money is a value object in Billing.

InvoiceAggregate is an aggregate in Billing.

Invoice has value object Money.

An invoice is rejected unless amount > 0.

## Ubiquitous language

- Invoice: a request for payment
",
    );
    domain_scan_with(tmp.path(), &clock()).unwrap();
    let store = KnowledgeStore::open(tmp.path()).unwrap();
    let nodes = store.domain_nodes().unwrap();
    let invoice = nodes
        .iter()
        .find(|node| node.id == "domain.billing.entity.Invoice")
        .unwrap();
    assert_eq!(invoice.status, "proposed");
    assert_eq!(invoice.provenance.kind, ProvenanceKind::Declared);
    let term = nodes
        .iter()
        .find(|node| node.id == "domain.billing.terminology.Invoice")
        .unwrap();
    assert_eq!(term.status, "observed");
    assert!(
        nodes
            .iter()
            .all(|node| node.status != "confirmed" && node.status != "canonical")
    );
    assert!(store.domain_edges().unwrap().iter().any(|edge| {
        edge.relation == RelationType::HasValueObject
            && edge.from_id == "domain.billing.entity.Invoice"
            && edge.to_id == "domain.billing.value_object.Money"
    }));
}

#[test]
fn migration_from_version_1_keeps_software_nodes() {
    let tmp = tempfile::tempdir().unwrap();
    touch(
        tmp.path(),
        "app/payments/models.py",
        "class Payment:\n    def __init__(self, payment_id: str) -> None:\n        self.payment_id = payment_id\n",
    );
    crate::scan(tmp.path()).unwrap();
    let store = KnowledgeStore::open(tmp.path()).unwrap();
    let before = store.software_nodes().unwrap().len();
    assert!(before > 0);
    assert_eq!(store.schema_version().unwrap(), 2);
    drop(store);
    let conn = rusqlite::Connection::open(tmp.path().join(".agentos/agentos.db")).unwrap();
    conn.execute_batch(
        "DROP TABLE domain_findings;
         DROP TABLE domain_transitions;
         DROP TABLE domain_edges;
         DROP TABLE domain_nodes;
         DELETE FROM knowledge_migrations WHERE version = 2;",
    )
    .unwrap();
    drop(conn);
    let store = KnowledgeStore::open(tmp.path()).unwrap();
    assert_eq!(store.schema_version().unwrap(), 2);
    assert_eq!(store.software_nodes().unwrap().len(), before);
    assert!(store.domain_nodes().unwrap().is_empty());
}

#[test]
fn generated_markdown_is_stable() {
    let tmp = tempfile::tempdir().unwrap();
    payments_project(tmp.path());
    domain_scan_with(tmp.path(), &clock()).unwrap();
    propose_with(tmp.path(), &crate::NullProposer, &clock()).unwrap();
    confirm_with(
        tmp.path(),
        "domain.payments.entity.Payment",
        "operator",
        "yes",
        &clock(),
    )
    .unwrap();
    let dir = tmp.path().join(".agentos/knowledge/domain");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    files.sort();
    let names: Vec<_> = files
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        vec![
            "aggregates.md",
            "bounded-contexts.md",
            "context-map.md",
            "invariants.md",
            "ubiquitous-language.md",
        ]
    );
    let text = fs::read_to_string(dir.join("bounded-contexts.md")).unwrap();
    assert!(text.starts_with("# Bounded contexts\n"));
    assert!(text.contains("domain.payments.entity.Payment") || text.contains("**Payments**"));
    assert!(!text.contains("CLOCK-SHOULD-NOT-APPEAR"));
    let again = fs::read_to_string(dir.join("bounded-contexts.md")).unwrap();
    domain_scan_with(tmp.path(), &clock()).unwrap();
    assert_eq!(
        again,
        fs::read_to_string(dir.join("bounded-contexts.md")).unwrap()
    );
}

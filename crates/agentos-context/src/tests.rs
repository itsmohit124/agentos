//! Contract tests. The engine is driven through [`MemoryView`] first, then SQLite.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use agentos_knowledge::{
    KnowledgeEdge, KnowledgeFragment, KnowledgeNode, NodeKind, Provenance, ProvenanceKind,
    RelationType, domain_scan, scan,
};

use crate::pack::ContextPack;
use crate::semantic::{SemanticHit, SemanticRetriever};
use crate::{ContextConfig, MemoryView, NullSemantic, build, render_json};

fn provenance(source: &str) -> Provenance {
    Provenance {
        kind: ProvenanceKind::Inferred,
        source: source.to_owned(),
        location: "L1-L2".to_owned(),
        extractor: "test".to_owned(),
        extractor_version: "1".to_owned(),
        method: "test".to_owned(),
    }
}

fn node(id: &str, kind: NodeKind, name: &str, status: &str, source: &str) -> KnowledgeNode {
    KnowledgeNode {
        id: id.to_owned(),
        node_type: kind,
        name: name.to_owned(),
        scope: String::new(),
        summary: name.to_owned(),
        status: status.to_owned(),
        provenance: provenance(source),
        confidence_millis: 700,
        version: 1,
        fingerprint: format!("fp-{id}"),
        source_refs: vec![source.to_owned()],
        retrieval_metadata: BTreeMap::new(),
        payload: BTreeMap::new(),
        source_id: source.to_owned(),
        parent: None,
    }
}

fn edge(from: &str, relation: RelationType, to: &str) -> KnowledgeEdge {
    KnowledgeEdge {
        id: format!("{from}|{}|{to}", relation.as_str()),
        from_id: from.to_owned(),
        relation,
        to_id: to.to_owned(),
        confidence_millis: 700,
        provenance: provenance("app/x.py"),
        metadata: BTreeMap::new(),
        source_id: "app/x.py".to_owned(),
    }
}

fn code_fragment(node_id: &str, source: &str, content: &str) -> KnowledgeFragment {
    KnowledgeFragment {
        id: format!("{node_id}#0"),
        node_id: node_id.to_owned(),
        source_ref: source.to_owned(),
        location: "L1-L4".to_owned(),
        content: content.to_owned(),
        fingerprint: format!("frag-{node_id}"),
        token_estimate: agentos_knowledge::token_estimate(content),
        kind: "code".to_owned(),
    }
}

fn payments_graph() -> MemoryView {
    let pay_ctx = "domain.payments.bounded_context.Payments";
    let pay = "domain.payments.entity.Payment";
    let note_ctx = "domain.notifications.bounded_context.Notifications";
    let note = "domain.notifications.domain_service.NotificationService";
    let mut view = MemoryView {
        fingerprint: "fp-a".to_owned(),
        node_reads: Arc::new(AtomicUsize::new(0)),
        ..MemoryView::default()
    };
    view.nodes = vec![
        node(
            pay_ctx,
            NodeKind::BoundedContext,
            "Payments",
            "inferred",
            "app/payments/models.py",
        ),
        node(
            pay,
            NodeKind::Entity,
            "Payment",
            "inferred",
            "app/payments/models.py",
        ),
        node(
            note_ctx,
            NodeKind::BoundedContext,
            "Notifications",
            "inferred",
            "app/notifications/service.py",
        ),
        node(
            note,
            NodeKind::DomainService,
            "NotificationService",
            "inferred",
            "app/notifications/service.py",
        ),
        node(
            "software.app.payments.models.class.Payment",
            NodeKind::Class,
            "Payment",
            "active",
            "app/payments/models.py",
        ),
        node(
            "software.app.notifications.service.class.NotificationService",
            NodeKind::Class,
            "NotificationService",
            "active",
            "app/notifications/service.py",
        ),
    ];
    view.edges = vec![
        edge(pay, RelationType::BelongsTo, pay_ctx),
        edge(note, RelationType::BelongsTo, note_ctx),
        edge(
            pay,
            RelationType::DerivedFrom,
            "software.app.payments.models.class.Payment",
        ),
        edge(
            note,
            RelationType::DerivedFrom,
            "software.app.notifications.service.class.NotificationService",
        ),
    ];
    view.fragments = vec![
        code_fragment(
            "software.app.payments.models.class.Payment",
            "app/payments/models.py",
            "class Payment:\n    pass\n",
        ),
        code_fragment(
            "software.app.notifications.service.class.NotificationService",
            "app/notifications/service.py",
            "class NotificationService:\n    def send(self, user_id: str) -> str:\n        return user_id\n",
        ),
    ];
    view
}

fn pack(view: &MemoryView, task: &str, budget: Option<u32>) -> ContextPack {
    build(
        view,
        task,
        &ContextConfig {
            budget,
            cache_dir: None,
        },
        &NullSemantic,
    )
    .expect("pack")
}

fn selected(pack: &ContextPack, id: &str) -> bool {
    pack.selected_nodes.iter().any(|node| node.id == id)
}

#[test]
fn intent_rules_are_deterministic() {
    let view = payments_graph();
    assert_eq!(
        pack(&view, "Add a refund method", None).intent.kind,
        "feature"
    );
    assert_eq!(
        pack(&view, "Fix the failing charge", None).intent.kind,
        "bugfix"
    );
    assert_eq!(
        pack(&view, "Refactor the payment repository", None)
            .intent
            .kind,
        "refactor"
    );
    assert_eq!(
        pack(&view, "Review the payment decision", None).intent.kind,
        "review"
    );
    assert_eq!(
        pack(&view, "Why must a payment amount be positive?", None)
            .intent
            .kind,
        "question"
    );
    let again = pack(&view, "Why must a payment amount be positive?", None);
    assert_eq!(again.intent.rule, "question-word:why");
}

#[test]
fn payment_task_does_not_select_notifications() {
    let view = payments_graph();
    let built = pack(
        &view,
        "Add retry support to payment webhook processing",
        None,
    );
    assert!(selected(&built, "domain.payments.bounded_context.Payments"));
    assert!(selected(&built, "domain.payments.entity.Payment"));
    assert!(!selected(
        &built,
        "domain.notifications.bounded_context.Notifications"
    ));
    assert!(!selected(
        &built,
        "software.app.notifications.service.class.NotificationService"
    ));
    let excluded = built
        .exclusions
        .iter()
        .find(|item| item.id == "domain.notifications.bounded_context.Notifications")
        .expect("notification exclusion");
    assert_eq!(excluded.reason, "outside_routed_context");
    assert!(!excluded.source.is_empty());
    assert!(excluded.token_estimate > 0);
    let trace = built
        .retrieval_trace
        .entries
        .iter()
        .find(|entry| entry.id == "domain.notifications.bounded_context.Notifications")
        .expect("trace");
    assert_eq!(trace.outcome, "excluded");
    assert!(!trace.reason.is_empty());
}

#[test]
fn service_task_routes_every_matching_context() {
    let mut view = payments_graph();
    view.nodes.push(node(
        "software.app.payments.service.class.PaymentService",
        NodeKind::Class,
        "PaymentService",
        "active",
        "app/payments/service.py",
    ));
    view.nodes.push(node(
        "domain.payments.domain_service.PaymentService",
        NodeKind::DomainService,
        "PaymentService",
        "inferred",
        "app/payments/service.py",
    ));
    view.nodes.push(node(
        "software.app.users.service.class.UserService",
        NodeKind::Class,
        "UserService",
        "active",
        "app/users/service.py",
    ));
    view.nodes.push(node(
        "domain.users.bounded_context.Users",
        NodeKind::BoundedContext,
        "Users",
        "inferred",
        "app/users/models.py",
    ));
    view.nodes.push(node(
        "domain.users.domain_service.UserService",
        NodeKind::DomainService,
        "UserService",
        "inferred",
        "app/users/service.py",
    ));
    let built = pack(&view, "Update the service layer", None);
    assert!(selected(&built, "domain.payments.bounded_context.Payments"));
    assert!(selected(&built, "domain.users.bounded_context.Users"));
    assert!(selected(
        &built,
        "domain.notifications.bounded_context.Notifications"
    ));
    assert!(built.intent.contexts.len() >= 3);
}

#[test]
fn proposed_and_inferred_status_are_not_rewritten() {
    let mut view = payments_graph();
    view.nodes.push(node(
        "domain.payments.terminology.Payment",
        NodeKind::Terminology,
        "Payment",
        "proposed",
        "docs/glossary.md",
    ));
    view.edges.push(edge(
        "domain.payments.terminology.Payment",
        RelationType::BelongsTo,
        "domain.payments.bounded_context.Payments",
    ));
    let built = pack(
        &view,
        "Add retry support to payment webhook processing",
        None,
    );
    let term = built
        .terminology
        .iter()
        .find(|entry| entry.id == "domain.payments.terminology.Payment")
        .expect("term");
    assert_eq!(term.status, "proposed");
    let entity = built
        .selected_nodes
        .iter()
        .find(|node| node.id == "domain.payments.entity.Payment")
        .expect("entity");
    assert_eq!(entity.status, "inferred");
}

#[test]
fn conflicts_are_listed_and_not_merged() {
    let mut view = payments_graph();
    view.nodes.push(node(
        "domain.payments.terminology.Payment",
        NodeKind::Terminology,
        "Payment",
        "inferred",
        "app/payments/models.py",
    ));
    view.nodes.push(node(
        "domain.payments.terminology.Charge",
        NodeKind::Terminology,
        "Charge",
        "inferred",
        "app/payments/models.py",
    ));
    view.edges.push(edge(
        "domain.payments.terminology.Payment",
        RelationType::ConflictsWith,
        "domain.payments.terminology.Charge",
    ));
    let built = pack(
        &view,
        "Add retry support to payment webhook processing",
        None,
    );
    let payment = built
        .terminology
        .iter()
        .find(|entry| entry.name == "Payment")
        .expect("payment term");
    assert_eq!(
        payment.conflicts,
        vec!["domain.payments.terminology.Charge".to_owned()]
    );
    assert!(built.terminology.iter().any(|entry| entry.name == "Charge"));
}

#[test]
fn over_budget_drops_code_before_domain_concepts() {
    let mut view = payments_graph();
    let class = "software.app.payments.models.class.Payment";
    view.fragments.retain(|fragment| fragment.node_id == class);
    if let Some(fragment) = view
        .fragments
        .iter_mut()
        .find(|fragment| fragment.node_id == class)
    {
        fragment.content = "x".repeat(400);
        fragment.token_estimate = 100;
    }
    let built = pack(
        &view,
        "Add retry support to payment webhook processing",
        Some(30),
    );
    assert!(selected(&built, "domain.payments.entity.Payment"));
    assert!(
        built
            .selected_fragments
            .iter()
            .all(|fragment| fragment.node_id != class)
            || built.budget.dropped > 0
    );
    let entity = built
        .selected_nodes
        .iter()
        .find(|node| node.id == "domain.payments.entity.Payment")
        .expect("entity kept");
    assert_eq!(entity.priority, 2);
    let class_node = built.selected_nodes.iter().find(|node| node.id == class);
    if let Some(class_node) = class_node {
        assert!(class_node.priority > entity.priority);
    }
    assert!(
        built
            .exclusions
            .iter()
            .any(|item| item.reason == "over_budget")
    );
}

#[test]
fn two_builds_are_byte_identical() {
    let view = payments_graph();
    let config = ContextConfig::default();
    let first = render_json(
        &view,
        "Add retry support to payment webhook processing",
        &config,
        &NullSemantic,
    )
    .expect("json");
    let second = render_json(
        &view,
        "Add retry support to payment webhook processing",
        &config,
        &NullSemantic,
    )
    .expect("json");
    assert_eq!(first, second);
    assert!(!first.contains("timestamp"));
    assert!(!first.contains("/Users/"));
}

struct OneHit;

impl SemanticRetriever for OneHit {
    fn search(&self, _query: &str, _nodes: &[KnowledgeNode]) -> Vec<SemanticHit> {
        vec![SemanticHit {
            node_id: "domain.notifications.bounded_context.Notifications".to_owned(),
            score: 80,
        }]
    }
}

#[test]
fn semantic_hit_outside_the_routed_context_is_excluded() {
    let view = payments_graph();
    let built = build(
        &view,
        "Add retry support to payment webhook processing",
        &ContextConfig::default(),
        &OneHit,
    )
    .expect("pack");
    assert!(!selected(
        &built,
        "domain.notifications.bounded_context.Notifications"
    ));
    let trace = built
        .retrieval_trace
        .entries
        .iter()
        .find(|entry| entry.id == "domain.notifications.bounded_context.Notifications")
        .expect("trace");
    assert_eq!(trace.reason, "semantic_outside_routed_context");
    assert_eq!(trace.score.semantic, 0);
}

#[test]
fn cache_hit_matches_a_fresh_build_and_invalidates() {
    let dir = tempfile::tempdir().expect("temp");
    let view = payments_graph();
    let config = ContextConfig {
        budget: None,
        cache_dir: Some(dir.path().to_path_buf()),
    };
    let fresh = render_json(
        &view,
        "Add retry support to payment webhook processing",
        &ContextConfig::default(),
        &NullSemantic,
    )
    .expect("fresh");
    let stored = render_json(
        &view,
        "Add retry support to payment webhook processing",
        &config,
        &NullSemantic,
    )
    .expect("store");
    assert_eq!(fresh, stored);
    view.node_reads.store(0, Ordering::SeqCst);
    let hit = render_json(
        &view,
        "Add retry support to payment webhook processing",
        &config,
        &NullSemantic,
    )
    .expect("hit");
    assert_eq!(hit, fresh);
    assert_eq!(view.node_reads.load(Ordering::SeqCst), 0);

    let mut changed = view.clone();
    changed.fingerprint = "fp-b".to_owned();
    changed.nodes[0].name = "PaymentsRenamed".to_owned();
    changed.node_reads.store(0, Ordering::SeqCst);
    let rebuilt = render_json(
        &changed,
        "Add retry support to payment webhook processing",
        &config,
        &NullSemantic,
    )
    .expect("rebuild");
    assert_ne!(rebuilt, fresh);
    assert!(changed.node_reads.load(Ordering::SeqCst) > 0);

    fs::remove_dir_all(dir.path()).expect("delete cache");
    let after_delete = render_json(
        &view,
        "Add retry support to payment webhook processing",
        &config,
        &NullSemantic,
    )
    .expect("after delete");
    assert_eq!(after_delete, fresh);
}

#[test]
fn pack_fields_match_the_contract() {
    let built = pack(
        &payments_graph(),
        "Add retry support to payment webhook processing",
        None,
    );
    assert!(!built.task.is_empty());
    assert!(!built.intent.kind.is_empty());
    assert!(built.budget.limit.is_none());
    assert!(!built.selected_nodes.is_empty());
    assert!(!built.selected_fragments.is_empty());
    assert!(!built.retrieval_trace.entries.is_empty());
    assert!(built.exclusions.iter().all(|item| !item.reason.is_empty()));
    assert!(
        built
            .retrieval_trace
            .entries
            .iter()
            .all(|entry| { !entry.reason.is_empty() && !entry.source.is_empty() })
    );
}

fn copy_fixture(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("mkdir");
    let mut entries: Vec<_> = fs::read_dir(from)
        .expect("read")
        .map(|entry| entry.expect("entry"))
        .collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name();
        if name == ".agentos" || name == ".git" {
            continue;
        }
        let target = to.join(&name);
        let file_type = entry.file_type().expect("type");
        if file_type.is_dir() {
            copy_fixture(&entry.path(), &target);
        } else if file_type.is_file() {
            fs::copy(entry.path(), &target).expect("copy");
        }
    }
}

#[test]
fn sqlite_payment_task_excludes_notifications() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/python-fastapi");
    let dir = tempfile::tempdir().expect("temp");
    copy_fixture(&fixture, dir.path());
    scan(dir.path()).expect("scan");
    domain_scan(dir.path()).expect("domain scan");
    let store = agentos_knowledge::KnowledgeStore::open(dir.path()).expect("open");
    let view = crate::SqliteView::new(&store);
    let built = build(
        &view,
        "Add retry support to payment webhook processing",
        &ContextConfig::default(),
        &NullSemantic,
    )
    .expect("pack");
    assert!(selected(&built, "domain.payments.bounded_context.Payments"));
    assert!(selected(&built, "domain.payments.entity.Payment"));
    assert!(selected(
        &built,
        "software.app.payments.service.class.PaymentService"
    ));
    assert!(!selected(
        &built,
        "domain.notifications.bounded_context.Notifications"
    ));
    assert!(!selected(&built, "domain.users.bounded_context.Users"));
    let entity = built
        .selected_nodes
        .iter()
        .find(|node| node.id == "domain.payments.entity.Payment")
        .expect("payment");
    assert_eq!(entity.status, "inferred");
    let again = render_json(
        &view,
        "Add retry support to payment webhook processing",
        &ContextConfig::default(),
        &NullSemantic,
    )
    .expect("second");
    let first = render_json(
        &view,
        "Add retry support to payment webhook processing",
        &ContextConfig::default(),
        &NullSemantic,
    )
    .expect("first");
    assert_eq!(first, again);
}

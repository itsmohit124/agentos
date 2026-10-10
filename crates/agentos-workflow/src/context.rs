//! One Context Pack per step, built by the Phase 4 engine.
//!
//! Dependency outputs are attached beside the pack. They are not written into the
//! pack schema, and they are passed only to steps that declare the dependency.

use std::collections::BTreeMap;

use agentos_context::{ContextConfig, ContextPack, KnowledgeView, NullSemantic, build};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::WorkflowError;

/// Builds a Context Pack for one step task.
pub trait ContextSource {
    /// Returns the pack for `task`. The task is the user task plus the step.
    fn build_pack(&self, task: &str) -> Result<ContextPack, WorkflowError>;
}

/// Phase 4 engine over an existing knowledge view.
pub struct EngineContext<V> {
    /// Knowledge the retriever may see.
    pub view: V,
    /// Budget and cache settings.
    pub config: ContextConfig,
}

impl<V: KnowledgeView> ContextSource for EngineContext<V> {
    fn build_pack(&self, task: &str) -> Result<ContextPack, WorkflowError> {
        build(&self.view, task, &self.config, &NullSemantic)
            .map_err(|err| WorkflowError::Context(err.to_string()))
    }
}

/// The pack a step receives, plus the dependency outputs it declared.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepContext {
    /// Hash of the Context Pack JSON.
    pub id: String,
    /// Phase 4 pack.
    pub pack: ContextPack,
    /// `step.output` values from direct dependencies.
    pub dependency_outputs: BTreeMap<String, String>,
}

/// Step task text. The user task is the first line and is not rewritten.
pub fn step_task(task: &str, step_id: &str, summary: &str) -> String {
    format!("{task}\n\nStep: {step_id}\n{summary}")
}

/// Wraps a pack and dependency outputs.
pub fn assemble(
    pack: ContextPack,
    dependency_outputs: BTreeMap<String, String>,
) -> Result<StepContext, WorkflowError> {
    let body =
        serde_json::to_string(&pack).map_err(|err| WorkflowError::Context(err.to_string()))?;
    Ok(StepContext {
        id: hex_sha256(&body)[..16].to_owned(),
        pack,
        dependency_outputs,
    })
}

fn hex_sha256(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

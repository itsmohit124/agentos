//! Generic adapter: files under `.agentos/adapters/generic/` and the portable skill index.

use crate::contract::AgentAdapter;
use crate::error::AdapterError;
use crate::instructions;
use crate::runtime;
use crate::skills;
use crate::types::{AdapterCapabilities, ManagedWrite, SkillExposure, Support, WritePolicy};

/// Portable adapter. It does not require Cursor, Claude Code, Codex, or a network.
#[derive(Debug, Default, Clone, Copy)]
pub struct GenericAdapter;

impl GenericAdapter {
    /// Adapter id.
    pub const ID: &'static str = "generic";
}

impl AgentAdapter for GenericAdapter {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            id: Self::ID.to_owned(),
            install: Support::Supported,
            skills: Support::Supported,
            context: Support::Supported,
            workflow: Support::Supported,
            feedback: Support::Supported,
            external_agent: Support::Unsupported,
        }
    }

    fn external_command(&self) -> Option<&'static str> {
        None
    }

    fn artifacts(&self, exposure: &SkillExposure) -> Result<Vec<ManagedWrite>, AdapterError> {
        let body = runtime::instructions_for(self.id(), exposure);
        let index = skills::skills_json(self.id(), exposure)?;
        Ok(vec![
            ManagedWrite {
                path: ".agentos/adapters/generic/INSTRUCTIONS.md".to_owned(),
                policy: WritePolicy::Markers {
                    prelude: String::new(),
                    body,
                },
            },
            ManagedWrite {
                path: ".agentos/adapters/generic/skills.json".to_owned(),
                policy: WritePolicy::Whole { contents: index },
            },
        ])
    }
}

/// Cursor rule prelude, shared so the cursor crate does not duplicate the frontmatter.
pub fn cursor_prelude() -> String {
    instructions::cursor_prelude()
}

/// Claude passthrough body for one skill.
pub fn claude_passthrough(skill: &crate::types::ExposedSkill) -> String {
    instructions::claude_passthrough(skill)
}

/// Whether `name` can be a single skill directory.
pub fn safe_skill_dir(name: &str) -> bool {
    instructions::safe_skill_dir(name)
}

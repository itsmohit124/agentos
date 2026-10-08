//! AgentOS skill system: portable `SKILL.md` packages, discovery, dependencies, routing and
//! permissions. Works without any LLM.
//!
//! ```text
//! SkillLoader::for_project(..).discover()  ->  Discovery { registry, diagnostics }
//! registry.resolve("tdd")                  ->  [interface-design, tdd]
//! SkillRouter.route(&registry, &request)   ->  ranked skills with reasons
//! skill.check(Capability::Shell)           ->  allow / deny + reason (default deny)
//! validate(&discovery)                     ->  every problem, including skill bodies
//! ```
//!
//! Progressive disclosure: discovery reads only front-matter. Bodies are read by
//! [`Skill::load_body`] and references by [`Skill::load_reference`], never earlier.

pub mod builtin;
mod diagnostic;
mod frontmatter;
mod graph;
mod install;
mod loader;
mod manifest;
mod registry;
mod resolve;
mod router;
mod schema;
pub mod text;
mod validate;

pub use diagnostic::{Diagnostic, Severity, codes};
pub use frontmatter::FrontMatterError;
pub use graph::{EdgeKind, GraphEdge, GraphNode, SkillGraph};
pub use install::{InstallError, InstallSource, Installed, install};
pub use loader::{
    AGENTOS_SKILLS_DIR, Discovery, Level, PROJECT_SKILLS_DIR, SKILL_FILE, SkillLoader,
    SkillLocation,
};
pub use manifest::{
    EvalExpectation, Invocation, InvocationMode, SkillDependency, SkillEvalCase, SkillIo,
    SkillKind, SkillManifest, SkillPermission, SkillVersion, Triggers, parse_manifest,
};
pub use registry::{Shadowed, Skill, SkillError, SkillRegistry};
pub use resolve::ResolveError;
pub use router::{
    INTENT_POINTS, Invoker, KEYWORD_POINTS, PHRASE_POINTS, RankedSkill, RejectedSkill, RouteError,
    RouteRequest, RouteResult, SkillRouter,
};
pub use schema::SKILL_MANIFEST_SCHEMA;
pub use validate::{REQUIRED_SECTIONS, ValidationReport, missing_sections, validate};

#[cfg(test)]
mod tests;

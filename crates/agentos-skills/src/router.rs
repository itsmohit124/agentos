//! Deterministic, rule-based skill routing. No LLM, no randomness.
//!
//! Each candidate skill is scored from its declared triggers against the normalised task text
//! (see [`crate::text::normalize`]):
//!
//! | Match | Points |
//! |---|---|
//! | request intent label in `triggers.intents` | [`INTENT_POINTS`] each |
//! | `triggers.phrases` entry, or the skill name, as a contiguous word run | [`PHRASE_POINTS`] each |
//! | `triggers.keywords` entry as a word | [`KEYWORD_POINTS`] each |
//!
//! Any `triggers.exclude` match disqualifies the skill. Skills with a positive score are ranked by
//! score (descending), then name (ascending). Every point is explained in `reasons`.

use serde::Serialize;

use crate::manifest::SkillKind;
use crate::registry::{Skill, SkillRegistry};
use crate::text::{contains_run, normalize};

/// Points for a matching intent label.
pub const INTENT_POINTS: u32 = 20;
/// Points for a matching phrase or skill name.
pub const PHRASE_POINTS: u32 = 10;
/// Points for a matching keyword.
pub const KEYWORD_POINTS: u32 = 3;

/// Who is asking for a skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Invoker {
    /// The human. Only user-invocable skills are candidates.
    User,
    /// The agent. Only model-invocable skills are candidates.
    Model,
}

impl Invoker {
    fn may_invoke(self, skill: &Skill) -> bool {
        let inv = skill.manifest().invocation;
        match self {
            Self::User => inv.user_invocable,
            Self::Model => inv.model_invocable,
        }
    }
}

/// A routing request.
#[derive(Debug, Clone)]
pub struct RouteRequest {
    /// Task text.
    pub task: String,
    /// Optional intent labels supplied by the caller.
    pub intents: Vec<String>,
    /// Who is asking.
    pub invoker: Invoker,
    /// Restrict candidates to the routes of this router skill.
    pub router: Option<String>,
}

impl RouteRequest {
    /// A request for `task` by `invoker`, with no intents and no router.
    pub fn new(task: impl Into<String>, invoker: Invoker) -> Self {
        Self {
            task: task.into(),
            intents: Vec::new(),
            invoker,
            router: None,
        }
    }
}

/// A ranked candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RankedSkill {
    /// Skill name.
    pub name: String,
    /// Total points.
    pub score: u32,
    /// One entry per contributing match, e.g. `phrase "write tests first" (+10)`.
    pub reasons: Vec<String>,
}

/// A skill that matched but is not eligible, with the reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RejectedSkill {
    /// Skill name.
    pub name: String,
    /// Why it was rejected.
    pub reason: String,
}

/// Routing outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RouteResult {
    /// Normalised task words the rules were matched against.
    pub tokens: Vec<String>,
    /// Eligible matches, best first.
    pub ranked: Vec<RankedSkill>,
    /// Matching but ineligible skills, sorted by name.
    pub rejected: Vec<RejectedSkill>,
}

impl RouteResult {
    /// Name of the best match.
    pub fn top(&self) -> Option<&str> {
        self.ranked.first().map(|r| r.name.as_str())
    }
}

/// Why routing could not run.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RouteError {
    /// The requested router is not registered.
    #[error("unknown router skill {0:?}")]
    UnknownRouter(String),
    /// The requested router is not of kind `router`.
    #[error("skill {0:?} is not a router")]
    NotARouter(String),
    /// The invoker may not use the requested router.
    #[error("router {0:?} is not invocable by the {1}")]
    RouterNotInvocable(String, &'static str),
}

/// The deterministic skill router.
#[derive(Debug, Default, Clone, Copy)]
pub struct SkillRouter;

impl SkillRouter {
    /// Ranks the skills of `registry` for `request`.
    pub fn route(
        &self,
        registry: &SkillRegistry,
        request: &RouteRequest,
    ) -> Result<RouteResult, RouteError> {
        let routes = match &request.router {
            None => None,
            Some(name) => {
                let router = registry
                    .get(name)
                    .ok_or_else(|| RouteError::UnknownRouter(name.clone()))?;
                if router.manifest().kind != SkillKind::Router {
                    return Err(RouteError::NotARouter(name.clone()));
                }
                if !request.invoker.may_invoke(router) {
                    let who = match request.invoker {
                        Invoker::User => "user",
                        Invoker::Model => "model",
                    };
                    return Err(RouteError::RouterNotInvocable(name.clone(), who));
                }
                Some(router.manifest().routes.clone())
            }
        };

        let tokens = normalize(&request.task);
        let intents: Vec<Vec<String>> = request.intents.iter().map(|i| normalize(i)).collect();
        let mut ranked = Vec::new();
        let mut rejected = Vec::new();
        for skill in registry.iter() {
            if skill.manifest().kind == SkillKind::Router {
                continue;
            }
            if routes
                .as_ref()
                .is_some_and(|r| !r.iter().any(|n| n == skill.name()))
            {
                continue;
            }
            let Some(score) = score(skill, &tokens, &intents) else {
                continue;
            };
            if let Some(reason) = score.excluded_by {
                rejected.push(RejectedSkill {
                    name: skill.name().to_owned(),
                    reason: format!("excluded by {reason:?}"),
                });
            } else if !request.invoker.may_invoke(skill) {
                rejected.push(RejectedSkill {
                    name: skill.name().to_owned(),
                    reason: match request.invoker {
                        Invoker::Model => "not model-invocable (user-invoked only)".to_owned(),
                        Invoker::User => "not user-invocable (model-invoked only)".to_owned(),
                    },
                });
            } else {
                ranked.push(RankedSkill {
                    name: skill.name().to_owned(),
                    score: score.points,
                    reasons: score.reasons,
                });
            }
        }
        ranked.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.name.cmp(&b.name)));
        Ok(RouteResult {
            tokens,
            ranked,
            rejected,
        })
    }
}

struct Score {
    points: u32,
    reasons: Vec<String>,
    excluded_by: Option<String>,
}

/// Scores `skill`; `None` if nothing matched.
fn score(skill: &Skill, tokens: &[String], intents: &[Vec<String>]) -> Option<Score> {
    let triggers = &skill.manifest().triggers;
    let mut points = 0;
    let mut reasons = Vec::new();

    let mut intent_labels: Vec<&String> = triggers.intents.iter().collect();
    intent_labels.sort();
    for label in intent_labels {
        if intents.contains(&normalize(label)) {
            points += INTENT_POINTS;
            reasons.push(format!("intent {label:?} (+{INTENT_POINTS})"));
        }
    }

    let name_phrase = skill.name().replace('-', " ");
    if contains_run(tokens, &normalize(&name_phrase)) {
        points += PHRASE_POINTS;
        reasons.push(format!("name {name_phrase:?} (+{PHRASE_POINTS})"));
    }
    let mut phrases: Vec<&String> = triggers.phrases.iter().collect();
    phrases.sort();
    for phrase in phrases {
        let words = normalize(phrase);
        if words != normalize(&name_phrase) && contains_run(tokens, &words) {
            points += PHRASE_POINTS;
            reasons.push(format!("phrase {phrase:?} (+{PHRASE_POINTS})"));
        }
    }

    let mut keywords: Vec<&String> = triggers.keywords.iter().collect();
    keywords.sort();
    for keyword in keywords {
        if normalize(keyword)
            .first()
            .is_some_and(|k| tokens.contains(k))
        {
            points += KEYWORD_POINTS;
            reasons.push(format!("keyword {keyword:?} (+{KEYWORD_POINTS})"));
        }
    }

    if points == 0 {
        return None;
    }
    let mut excludes: Vec<&String> = triggers.exclude.iter().collect();
    excludes.sort();
    let excluded_by = excludes
        .into_iter()
        .find(|e| contains_run(tokens, &normalize(e)))
        .cloned();
    Some(Score {
        points,
        reasons,
        excluded_by,
    })
}

//! Domain lifecycle.
//!
//! `Observed → Inferred → Proposed → Confirmed → Canonical`.
//! Inference and proposal stop at Proposed. Only [`crate::domain::confirm`] enters
//! Confirmed, and only a second confirm enters Canonical.

use crate::error::KnowledgeError;

/// Belief state of one domain node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lifecycle {
    /// A term or label read from a project artifact, not yet interpreted.
    Observed,
    /// A heuristic classification. Not project truth.
    Inferred,
    /// A suggestion awaiting a person.
    Proposed,
    /// Accepted by `domain confirm`.
    Confirmed,
    /// Promoted by a second `domain confirm` while evidence still exists.
    Canonical,
    /// Rejected by `domain reject`. Terminal.
    Rejected,
}

impl Lifecycle {
    /// Wire name stored in `nodes.status`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Observed => "observed",
            Self::Inferred => "inferred",
            Self::Proposed => "proposed",
            Self::Confirmed => "confirmed",
            Self::Canonical => "canonical",
            Self::Rejected => "rejected",
        }
    }

    /// Parses a wire name.
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "observed" => Self::Observed,
            "inferred" => Self::Inferred,
            "proposed" => Self::Proposed,
            "confirmed" => Self::Confirmed,
            "canonical" => Self::Canonical,
            "rejected" => Self::Rejected,
            _ => return None,
        })
    }

    /// Position on the forward chain. Rejected is not on the chain.
    pub fn rank(self) -> Option<u8> {
        Some(match self {
            Self::Observed => 0,
            Self::Inferred => 1,
            Self::Proposed => 2,
            Self::Confirmed => 3,
            Self::Canonical => 4,
            Self::Rejected => return None,
        })
    }

    /// The next forward state, if this state has one.
    pub fn forward(self) -> Option<Self> {
        Some(match self {
            Self::Observed => Self::Inferred,
            Self::Inferred => Self::Proposed,
            Self::Proposed => Self::Confirmed,
            Self::Confirmed => Self::Canonical,
            Self::Canonical | Self::Rejected => return None,
        })
    }

    /// True when `domain reject` may leave this state.
    pub fn can_reject(self) -> bool {
        matches!(
            self,
            Self::Observed | Self::Inferred | Self::Proposed | Self::Confirmed
        )
    }

    /// Every state, in chain order, with Rejected last.
    pub fn all() -> &'static [Self] {
        &[
            Self::Observed,
            Self::Inferred,
            Self::Proposed,
            Self::Confirmed,
            Self::Canonical,
            Self::Rejected,
        ]
    }
}

/// Where a newly created domain node is allowed to start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Folder, name, or text heuristic.
    Heuristic,
    /// A bullet in a ubiquitous-language section.
    Glossary,
    /// A requirements or design sentence in a greenfield (or spec) artifact.
    Requirement,
    /// [`crate::domain::DomainProposer`].
    Proposer,
}

impl Origin {
    /// The only status this origin may create.
    pub fn initial(self) -> Lifecycle {
        match self {
            Self::Heuristic => Lifecycle::Inferred,
            Self::Glossary => Lifecycle::Observed,
            Self::Requirement | Self::Proposer => Lifecycle::Proposed,
        }
    }
}

/// Refuses the change unless it is one allowed step.
pub fn check(from: Lifecycle, to: Lifecycle) -> Result<(), KnowledgeError> {
    let allowed = matches!(
        (from, to),
        (Lifecycle::Observed, Lifecycle::Inferred)
            | (Lifecycle::Inferred, Lifecycle::Proposed)
            | (Lifecycle::Proposed, Lifecycle::Confirmed)
            | (Lifecycle::Confirmed, Lifecycle::Canonical)
            | (Lifecycle::Observed, Lifecycle::Rejected)
            | (Lifecycle::Inferred, Lifecycle::Rejected)
            | (Lifecycle::Proposed, Lifecycle::Rejected)
            | (Lifecycle::Confirmed, Lifecycle::Rejected)
    );
    if allowed {
        Ok(())
    } else {
        Err(KnowledgeError::IllegalTransition {
            from: from.as_str().to_owned(),
            to: to.as_str().to_owned(),
        })
    }
}

/// True when a machine (scan, propose, a proposer) is not allowed to write `status`.
pub fn machine_status_forbidden(status: Lifecycle) -> bool {
    matches!(status, Lifecycle::Confirmed | Lifecycle::Canonical)
}

/// Keep a stored status when a later scan would otherwise move it backwards or off Rejected.
pub fn preserve_status(stored: Lifecycle, refreshed: Lifecycle) -> bool {
    match stored {
        Lifecycle::Rejected | Lifecycle::Confirmed | Lifecycle::Canonical => true,
        other => match (other.rank(), refreshed.rank()) {
            (Some(left), Some(right)) => left > right,
            _ => false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pair_matches_the_allowed_set() {
        let allowed = [
            (Lifecycle::Observed, Lifecycle::Inferred),
            (Lifecycle::Inferred, Lifecycle::Proposed),
            (Lifecycle::Proposed, Lifecycle::Confirmed),
            (Lifecycle::Confirmed, Lifecycle::Canonical),
            (Lifecycle::Observed, Lifecycle::Rejected),
            (Lifecycle::Inferred, Lifecycle::Rejected),
            (Lifecycle::Proposed, Lifecycle::Rejected),
            (Lifecycle::Confirmed, Lifecycle::Rejected),
        ];
        for from in Lifecycle::all() {
            for to in Lifecycle::all() {
                let result = check(*from, *to);
                let expect = allowed.contains(&(*from, *to));
                assert_eq!(
                    result.is_ok(),
                    expect,
                    "{:?} → {:?} → {result:?}",
                    from.as_str(),
                    to.as_str()
                );
            }
        }
    }

    #[test]
    fn inference_and_proposal_origins_cannot_start_confirmed() {
        for origin in [
            Origin::Heuristic,
            Origin::Glossary,
            Origin::Requirement,
            Origin::Proposer,
        ] {
            let status = origin.initial();
            assert!(!machine_status_forbidden(status), "{}", status.as_str());
            assert_ne!(status, Lifecycle::Confirmed);
            assert_ne!(status, Lifecycle::Canonical);
        }
        assert!(machine_status_forbidden(Lifecycle::Confirmed));
        assert!(machine_status_forbidden(Lifecycle::Canonical));
    }

    #[test]
    fn forward_steps_are_the_chain_and_reject_stops_at_canonical() {
        assert_eq!(Lifecycle::Observed.forward(), Some(Lifecycle::Inferred));
        assert_eq!(Lifecycle::Inferred.forward(), Some(Lifecycle::Proposed));
        assert_eq!(Lifecycle::Proposed.forward(), Some(Lifecycle::Confirmed));
        assert_eq!(Lifecycle::Confirmed.forward(), Some(Lifecycle::Canonical));
        assert_eq!(Lifecycle::Canonical.forward(), None);
        assert_eq!(Lifecycle::Rejected.forward(), None);
        assert!(!Lifecycle::Canonical.can_reject());
        assert!(!Lifecycle::Rejected.can_reject());
        assert!(Lifecycle::Confirmed.can_reject());
    }

    #[test]
    fn rescan_does_not_demote_confirmed_or_proposed() {
        assert!(preserve_status(Lifecycle::Confirmed, Lifecycle::Inferred));
        assert!(preserve_status(Lifecycle::Canonical, Lifecycle::Inferred));
        assert!(preserve_status(Lifecycle::Rejected, Lifecycle::Inferred));
        assert!(preserve_status(Lifecycle::Proposed, Lifecycle::Inferred));
        assert!(!preserve_status(Lifecycle::Inferred, Lifecycle::Inferred));
        assert!(!preserve_status(Lifecycle::Observed, Lifecycle::Inferred));
    }
}

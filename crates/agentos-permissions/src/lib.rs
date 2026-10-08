//! AgentOS default-deny permission model and enforcement.
//!
//! A [`PermissionSubject`] (for example a skill) declares a [`PermissionSet`] of [`Capability`]s.
//! [`check`] is the single enforcement point: it allows a capability only if the subject declared
//! it. Nothing is granted by default, capabilities never imply each other, and grants are not
//! inherited from other subjects.

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A capability a subject may be granted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Capability {
    /// Read files in the project.
    FsRead,
    /// Create, modify or delete files in the project.
    FsWrite,
    /// Run shell commands.
    Shell,
    /// Make network requests.
    Network,
    /// Drive a browser.
    Browser,
}

impl Capability {
    /// Every capability, in canonical order.
    pub const ALL: [Self; 5] = [
        Self::FsRead,
        Self::FsWrite,
        Self::Shell,
        Self::Network,
        Self::Browser,
    ];

    /// Stable identifier (e.g. `"fs.read"`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FsRead => "fs.read",
            Self::FsWrite => "fs.write",
            Self::Shell => "shell",
            Self::Network => "network",
            Self::Browser => "browser",
        }
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error returned when parsing an unknown capability identifier.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown capability {0:?} (known: fs.read, fs.write, shell, network, browser)")]
pub struct UnknownCapability(pub String);

impl FromStr for Capability {
    type Err = UnknownCapability;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|c| c.as_str() == s)
            .ok_or_else(|| UnknownCapability(s.to_owned()))
    }
}

impl Serialize for Capability {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Capability {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// The capabilities a subject declared. Empty by default (default-deny).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PermissionSet(BTreeSet<Capability>);

impl PermissionSet {
    /// An empty set: every capability is denied.
    pub fn none() -> Self {
        Self::default()
    }

    /// Whether `capability` was declared.
    pub fn contains(&self, capability: Capability) -> bool {
        self.0.contains(&capability)
    }

    /// Declared capabilities in canonical order.
    pub fn iter(&self) -> impl Iterator<Item = Capability> + '_ {
        self.0.iter().copied()
    }

    /// Whether nothing was declared.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl FromIterator<Capability> for PermissionSet {
    fn from_iter<I: IntoIterator<Item = Capability>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

/// Anything permissions can be checked for.
pub trait PermissionSubject {
    /// Human-readable identity used in decision reasons (e.g. `"skill 'tdd'"`).
    fn subject(&self) -> String;

    /// Capabilities the subject declared.
    fn permissions(&self) -> &PermissionSet;
}

/// Outcome of a permission check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    /// The capability is granted.
    Allow,
    /// The capability is denied.
    Deny,
}

/// A permission decision with the reason it was made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Decision {
    /// Allow or deny.
    pub verdict: Verdict,
    /// The capability that was checked.
    pub capability: Capability,
    /// Why.
    pub reason: String,
}

impl Decision {
    /// Whether the capability is granted.
    pub fn is_allowed(&self) -> bool {
        self.verdict == Verdict::Allow
    }
}

/// Checks whether `subject` may use `capability`.
///
/// Allows only capabilities the subject declared itself; everything else is denied.
pub fn check(subject: &dyn PermissionSubject, capability: Capability) -> Decision {
    if subject.permissions().contains(capability) {
        Decision {
            verdict: Verdict::Allow,
            capability,
            reason: format!("{} declares {capability}", subject.subject()),
        }
    } else {
        Decision {
            verdict: Verdict::Deny,
            capability,
            reason: format!(
                "{} does not declare {capability} (default deny)",
                subject.subject()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Subject(PermissionSet);

    impl PermissionSubject for Subject {
        fn subject(&self) -> String {
            "skill 'x'".to_owned()
        }
        fn permissions(&self) -> &PermissionSet {
            &self.0
        }
    }

    #[test]
    fn empty_set_denies_everything() {
        let s = Subject(PermissionSet::none());
        for cap in Capability::ALL {
            let d = check(&s, cap);
            assert_eq!(d.verdict, Verdict::Deny, "{cap}");
            assert_eq!(
                d.reason,
                format!("skill 'x' does not declare {cap} (default deny)")
            );
        }
    }

    #[test]
    fn only_declared_capabilities_are_allowed() {
        let s = Subject(
            [Capability::FsRead, Capability::Network]
                .into_iter()
                .collect(),
        );
        let allowed: Vec<Capability> = Capability::ALL
            .into_iter()
            .filter(|c| check(&s, *c).is_allowed())
            .collect();
        assert_eq!(allowed, [Capability::FsRead, Capability::Network]);
        assert_eq!(
            check(&s, Capability::FsRead).reason,
            "skill 'x' declares fs.read"
        );
    }

    #[test]
    fn capabilities_do_not_imply_each_other() {
        let s = Subject([Capability::FsWrite].into_iter().collect());
        assert!(!check(&s, Capability::FsRead).is_allowed());
        let s = Subject([Capability::Shell].into_iter().collect());
        assert!(!check(&s, Capability::FsWrite).is_allowed());
        assert!(!check(&s, Capability::Network).is_allowed());
    }

    #[test]
    fn capability_round_trips_and_rejects_unknown() {
        for cap in Capability::ALL {
            assert_eq!(cap.as_str().parse::<Capability>().unwrap(), cap);
        }
        assert_eq!(
            "teleport".parse::<Capability>().unwrap_err(),
            UnknownCapability("teleport".into())
        );
        assert!("FS.READ".parse::<Capability>().is_err());
    }

    #[test]
    fn serde_uses_identifiers() {
        let set: PermissionSet = serde_json::from_str(r#"["shell","fs.read","shell"]"#).unwrap();
        assert_eq!(
            serde_json::to_string(&set).unwrap(),
            r#"["fs.read","shell"]"#
        );
        assert!(serde_json::from_str::<PermissionSet>(r#"["root"]"#).is_err());
    }
}

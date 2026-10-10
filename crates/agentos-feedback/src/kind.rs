//! Validation kinds. Technology-specific providers choose which ones they implement.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// What a validation run is asking for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationKind {
    /// Static type check.
    Typecheck,
    /// Lint or clippy-style check.
    Lint,
    /// Unit tests.
    UnitTest,
    /// Integration tests.
    IntegrationTest,
    /// Compile or package the project.
    Build,
    /// Architecture detectors. The shell is not required.
    Architecture,
}

impl ValidationKind {
    /// Stable wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Typecheck => "typecheck",
            Self::Lint => "lint",
            Self::UnitTest => "unit_test",
            Self::IntegrationTest => "integration_test",
            Self::Build => "build",
            Self::Architecture => "architecture",
        }
    }

    /// Every kind, in a fixed order.
    pub const ALL: [Self; 6] = [
        Self::Typecheck,
        Self::Lint,
        Self::UnitTest,
        Self::IntegrationTest,
        Self::Build,
        Self::Architecture,
    ];
}

impl fmt::Display for ValidationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ValidationKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == s)
            .ok_or_else(|| format!("unknown validation kind {s}"))
    }
}

//! Concrete validation providers.
//!
//! The trait surface is [`crate::ValidationProvider`]. These modules own command lines and
//! output grammars for each toolchain.

mod cargo;
mod go;
mod gradle;
mod jest;
mod maven;
mod pytest;
mod rspec;

pub use cargo::CargoTestProvider;
pub use go::GoTestProvider;
pub use gradle::GradleProvider;
pub use jest::JestProvider;
pub use maven::MavenProvider;
pub use pytest::PytestProvider;
pub use rspec::RSpecProvider;

use crate::provider::ValidationProvider;

/// Every built-in provider, sorted by id.
pub fn builtin_providers() -> Vec<Box<dyn ValidationProvider>> {
    vec![
        Box::new(CargoTestProvider),
        Box::new(GoTestProvider),
        Box::new(GradleProvider),
        Box::new(JestProvider),
        Box::new(MavenProvider),
        Box::new(PytestProvider),
        Box::new(RSpecProvider),
    ]
}

/// Looks up a provider id.
pub fn provider_by_id(id: &str) -> Option<Box<dyn ValidationProvider>> {
    builtin_providers()
        .into_iter()
        .find(|provider| provider.id() == id)
}

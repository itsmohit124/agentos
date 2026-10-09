//! Language-specific extractors. Core node types do not name these languages.

mod go;
mod java;
mod python;
mod ruby;
mod typescript;
mod util;

pub use go::GoExtractor;
pub use java::JavaExtractor;
pub use python::PythonExtractor;
pub use ruby::RubyExtractor;
pub use typescript::{JavaScriptExtractor, TypeScriptExtractor};

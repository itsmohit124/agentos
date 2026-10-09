//! Language-specific extractors. Core node types do not name these languages.

mod python;
mod typescript;
mod util;

pub use python::PythonExtractor;
pub use typescript::{JavaScriptExtractor, TypeScriptExtractor};

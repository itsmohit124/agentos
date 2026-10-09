//! Language extractors.
//!
//! The trait is the extension point. Python and TypeScript/JavaScript ship in this crate
//! behind it; another language registers an implementation and does not change node types.

use std::path::Path;

use crate::error::KnowledgeError;
use crate::model::NodeKind;

/// A call site recorded by an extractor. Resolution happens later, language-agnostically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawCall {
    /// Method or function name that was called.
    pub callee: String,
    /// Receiver expression when the call is `receiver.callee(...)`.
    pub receiver: Option<String>,
    /// Type of the receiver when a parameter annotation names it.
    pub receiver_type: Option<String>,
    /// 1-based line of the call.
    pub line: u32,
}

/// One symbol parsed out of a source file. File and module nodes are added by the scanner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawSymbol {
    /// Class, function, method, import, interface, or symbol.
    pub kind: NodeKind,
    /// Local name. Methods use the method name, not `Class.method`.
    pub name: String,
    /// Enclosing class or interface for methods.
    pub parent: Option<String>,
    /// 1-based start line.
    pub start_line: u32,
    /// 1-based end line.
    pub end_line: u32,
    /// One-line summary.
    pub summary: String,
    /// Source slice of the symbol.
    pub body: String,
    /// Base types named in the source (`class Foo(Bar)`).
    pub bases: Vec<String>,
    /// Interfaces named in the source (`implements Bar`).
    pub implements: Vec<String>,
    /// Calls inside this symbol.
    pub calls: Vec<RawCall>,
    /// Imported module path, for [`NodeKind::Import`].
    pub import_target: Option<String>,
}

/// Parses one language into symbols.
pub trait LanguageExtractor: Send + Sync {
    /// Stable extractor name recorded on provenance.
    fn name(&self) -> &'static str;
    /// Bumped when the extractor's rules change.
    fn version(&self) -> &'static str;
    /// Whether this extractor wants `path`.
    fn matches(&self, path: &Path) -> bool;
    /// Symbols in `source`. `relative` is the `/`-separated path (used only for diagnostics).
    fn extract(&self, relative: &str, source: &str) -> Result<Vec<RawSymbol>, KnowledgeError>;
}

/// Ordered list of extractors. The first match wins.
#[derive(Default)]
pub struct ExtractorRegistry {
    extractors: Vec<Box<dyn LanguageExtractor>>,
}

impl ExtractorRegistry {
    /// Empty registry. Tests register a stub to prove the trait is the extension point.
    pub fn new() -> Self {
        Self::default()
    }

    /// Python, TypeScript, and JavaScript.
    pub fn builtin() -> Self {
        let mut registry = Self::new();
        registry.register(Box::new(crate::languages::PythonExtractor));
        registry.register(Box::new(crate::languages::TypeScriptExtractor));
        registry.register(Box::new(crate::languages::JavaScriptExtractor));
        registry
    }

    /// Adds an extractor. Later registrations are tried after earlier ones.
    pub fn register(&mut self, extractor: Box<dyn LanguageExtractor>) {
        self.extractors.push(extractor);
    }

    /// First extractor that matches `path`.
    pub fn find(&self, path: &Path) -> Option<&dyn LanguageExtractor> {
        self.extractors
            .iter()
            .find(|extractor| extractor.matches(path))
            .map(|extractor| extractor.as_ref())
    }
}

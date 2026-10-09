//! Stable logical ids.
//!
//! Ids are built from the language-qualified name, not from the repository path.
//! `src/` and `lib/` are source roots: moving `src/payments/models.py` to
//! `lib/payments/models.py` keeps `software.payments.models.class.Payment`.
//! Moving a file into a different package changes the qualified name, so the id changes.
//! That is a rename, not a move of the same module.

/// `software` for production code, `quality` for tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// Production source.
    Software,
    /// Tests.
    Quality,
}

impl Layer {
    /// Wire name used as the first id segment.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Software => "software",
            Self::Quality => "quality",
        }
    }
}

/// Module identity of one source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModulePath {
    /// `software` or `quality`.
    pub layer: Layer,
    /// Dotted module path (`app.payments.service`, `tests.test_payments`).
    pub module: String,
}

/// True when `relative` (forward slashes) is a test file.
pub fn is_test_path(relative: &str) -> bool {
    let file = file_name(relative);
    let parts: Vec<&str> = relative.split('/').collect();
    let in_test_dir = parts
        .iter()
        .any(|p| matches!(*p, "test" | "tests" | "__tests__"));
    let name_is_test = file.starts_with("test_")
        || file.ends_with("_test.py")
        || file.ends_with("_test.go")
        || file.ends_with("_test.rs")
        || file.ends_with(".test.ts")
        || file.ends_with(".test.tsx")
        || file.ends_with(".test.js")
        || file.ends_with(".test.jsx")
        || file.ends_with(".spec.ts")
        || file.ends_with(".spec.tsx")
        || file.ends_with(".spec.js")
        || file.ends_with(".spec.jsx");
    in_test_dir || name_is_test
}

/// Qualified module of a source file.
///
/// `__init__.py` names its directory. `src/` and `lib/` are stripped once, from the front.
pub fn module_path(relative: &str) -> ModulePath {
    let mut parts: Vec<&str> = relative.split('/').filter(|p| !p.is_empty()).collect();
    let file = parts.pop().unwrap_or(relative);
    if matches!(parts.first().copied(), Some("src" | "lib")) {
        parts.remove(0);
    }
    let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file);
    if stem != "__init__" && stem != "mod" {
        parts.push(stem);
    }
    let module = if parts.is_empty() {
        stem.to_owned()
    } else {
        parts.join(".")
    };
    ModulePath {
        layer: if is_test_path(relative) {
            Layer::Quality
        } else {
            Layer::Software
        },
        module,
    }
}

/// Logical id for a file or module node (no extra name).
pub fn container_id(module: &ModulePath, kind: &str) -> String {
    format!("{}.{}.{kind}", module.layer.as_str(), module.module)
}

/// Logical id for a named symbol.
///
/// `qualifier` is the symbol name, or `Class.method` for methods.
pub fn symbol_id(module: &ModulePath, kind: &str, qualifier: &str) -> String {
    format!(
        "{}.{}.{kind}.{qualifier}",
        module.layer.as_str(),
        module.module
    )
}

/// Document or decision id from a relative markdown path.
pub fn document_id(kind: &str, relative: &str) -> String {
    let stem = relative
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(relative);
    let dotted = stem.replace('/', ".");
    format!("{kind}.{dotted}")
}

fn file_name(relative: &str) -> &str {
    relative.rsplit('/').next().unwrap_or(relative)
}

/// True when `relative` is an architecture decision record.
pub fn is_adr(relative: &str) -> bool {
    relative
        .split('/')
        .any(|part| part == "adr" || part == "adrs" || part == "ADR")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_root_move_keeps_the_module() {
        let from_src = module_path("src/payments/models.py");
        let from_lib = module_path("lib/payments/models.py");
        assert_eq!(from_src, from_lib);
        assert_eq!(
            symbol_id(&from_src, "class", "Payment"),
            "software.payments.models.class.Payment"
        );
    }

    #[test]
    fn app_package_is_part_of_the_module() {
        let module = module_path("app/payments/service.py");
        assert_eq!(module.module, "app.payments.service");
        assert_eq!(
            symbol_id(&module, "class", "PaymentService"),
            "software.app.payments.service.class.PaymentService"
        );
        assert_eq!(
            symbol_id(&module, "method", "PaymentService.charge"),
            "software.app.payments.service.method.PaymentService.charge"
        );
    }

    #[test]
    fn tests_use_the_quality_layer() {
        let module = module_path("tests/test_payments.py");
        assert_eq!(module.layer, Layer::Quality);
        assert_eq!(
            symbol_id(&module, "test", "test_charge"),
            "quality.tests.test_payments.test.test_charge"
        );
    }

    #[test]
    fn init_files_name_the_package() {
        let module = module_path("app/payments/__init__.py");
        assert_eq!(module.module, "app.payments");
    }

    #[test]
    fn adr_paths_are_decisions() {
        assert!(is_adr("docs/adr/0001-record-payments.md"));
        assert!(!is_adr("docs/overview.md"));
    }
}

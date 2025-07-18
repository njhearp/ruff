use ruff_macros::{ViolationMetadata, derive_message_formats};
use ruff_python_semantic::Scope;
use ruff_text_size::Ranged;

use crate::Violation;
use crate::checkers::ast::Checker;

/// ## What it does
/// Checks for local variables that are annotated but never used.
///
/// ## Why is this bad?
/// Annotations are used to provide type hints to static type checkers. If a
/// variable is annotated but never used, the annotation is unnecessary.
///
/// ## Example
/// ```python
/// def foo():
///     bar: int
/// ```
///
/// ## References
/// - [PEP 484 – Type Hints](https://peps.python.org/pep-0484/)
#[derive(ViolationMetadata)]
pub(crate) struct UnusedAnnotation {
    name: String,
}

impl Violation for UnusedAnnotation {
    #[derive_message_formats]
    fn message(&self) -> String {
        let UnusedAnnotation { name } = self;
        format!("Local variable `{name}` is annotated but never used")
    }
}

/// F842
pub(crate) fn unused_annotation(checker: &Checker, scope: &Scope) {
    for (name, range) in scope.bindings().filter_map(|(name, binding_id)| {
        let binding = checker.semantic().binding(binding_id);
        if binding.kind.is_annotation()
            && binding.is_unused()
            && !checker.settings().dummy_variable_rgx.is_match(name)
        {
            Some((name.to_string(), binding.range()))
        } else {
            None
        }
    }) {
        checker.report_diagnostic(UnusedAnnotation { name }, range);
    }
}

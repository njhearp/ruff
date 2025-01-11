use ruff_diagnostics::{Diagnostic, Violation};
use ruff_macros::{derive_message_formats, ViolationMetadata};
use ruff_python_ast::{self as ast};
use ruff_text_size::Ranged;

use crate::checkers::ast::Checker;

use super::helpers::is_pytest_warns;

/// ## What it does
/// Checks for `pytest.warns` not passing a warning class.
///
/// ## Why is this bad?
/// Not passing the warning class will fail at runtime.
///
/// ## Example
/// ```python
/// import pytest
///
///
/// def test_foo():
///     with pytest.warns():
///         do_something()
/// ```
///
/// Use instead:
/// ```python
/// import pytest
///
///
/// def test_foo():
///     with pytest.warns(SomeWarning):
///         do_something()
/// ```
///
/// ## References
/// - [`pytest` documentation: `pytest.fail`](https://docs.pytest.org/en/latest/reference/reference.html#pytest-warns)
#[derive(ViolationMetadata)]
pub(crate) struct PytestWarnsWithoutWarning;

impl Violation for PytestWarnsWithoutWarning {
    #[derive_message_formats]
    fn message(&self) -> String {
        "`pytest.warns()` should pass a warning class".to_string()
    }
}

// PT029
pub(crate) fn without_warning(checker: &mut Checker, call: &ast::ExprCall) {
    if is_pytest_warns(&call.func, checker.semantic()) {
        if call.arguments.is_empty() {
            checker.diagnostics.push(Diagnostic::new(
                PytestWarnsWithoutWarning,
                call.func.range(),
            ));
        }
    }
}

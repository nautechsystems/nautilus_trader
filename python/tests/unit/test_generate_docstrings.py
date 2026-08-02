from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

import pytest


def _load_generate_docstrings_module():
    module_path = Path(__file__).resolve().parents[2] / "generate_docstrings.py"
    module_name = "generate_docstrings_module"
    spec = importlib.util.spec_from_file_location(module_name, module_path)
    module = importlib.util.module_from_spec(spec)
    assert spec is not None
    assert spec.loader is not None
    sys.modules[module_name] = module
    spec.loader.exec_module(module)
    return module


generate_docstrings = _load_generate_docstrings_module()


@pytest.mark.parametrize(
    ("line", "expected"),
    [
        ("#[new]", True),
        ("#[allow(unused_imports)]", True),
        ("#[allow(unused_imports)] // Used in template pattern", True),
        (")]", True),
        (")] // trailing comment", True),
        ("#[cfg_attr(", False),
        ('feature = "python",', False),
    ],
)
def test_attr_end_re_matches_closing_attribute_lines(line, expected):
    # Act
    result = generate_docstrings.ATTR_END_RE.search(line) is not None

    # Assert
    assert result is expected


def test_parse_pyo3_items_tolerates_attribute_with_trailing_comment():
    # Arrange: the trailing comment must not swallow the `#[pymethods]` marker
    lines = [
        "#[allow(unused_imports)] // Used in template pattern",
        "use nautilus_core::UnixNanos;",
        "",
        "#[pymethods]",
        "impl LongRatio {",
        "    #[new]",
        "    fn py_new() -> Self {",
        "        Self::new()",
        "    }",
        "}",
    ]

    # Act
    items = generate_docstrings.parse_pyo3_items(lines)

    # Assert
    assert len(items) == 1
    assert items[0]["fn_name"] == "py_new"
    assert items[0]["impl_type"] == "LongRatio"
    assert items[0]["is_constructor"] is True
    assert items[0]["in_pymethods"] is True


def test_parse_pyo3_items_captures_multiline_result_signature():
    # Arrange
    lines = [
        "#[pymethods]",
        "impl HttpClient {",
        '    #[pyo3(name = "request")]',
        "    fn py_request<'py>(",
        "        &self,",
        "        py: Python<'py>,",
        "    ) -> PyResult<Bound<'py, PyAny>> {",
        "        todo!()",
        "    }",
        "}",
    ]

    # Act
    items = generate_docstrings.parse_pyo3_items(lines)

    # Assert
    assert len(items) == 1
    assert items[0]["fn_signature"].endswith(") -> PyResult<Bound<'py, PyAny>> {")
    assert generate_docstrings.rust_fn_returns_result(items[0]["fn_signature"]) is True


@pytest.mark.parametrize(
    ("signature", "expected"),
    [
        ("fn py_reset(&mut self) {", False),
        ("fn py_new(value: ResultLikeName) -> Self {", False),
        ("fn py_body<'py>(&self) -> Bound<'py, PyAny> {", False),
        ("fn py_get(&self) -> anyhow::Result<()> {", True),
        ("fn py_get<T>(&self) -> PyResult<T> where T: Clone {", True),
    ],
)
def test_rust_fn_returns_result_detects_return_type_only(signature, expected):
    # Act
    result = generate_docstrings.rust_fn_returns_result(signature)

    # Assert
    assert result is expected


def test_transform_doc_drops_panics_without_warning(capsys):
    # Arrange
    doc_lines = [
        "Does work.",
        "",
        "# Errors",
        "",
        "Returns an error when the request fails.",
        "",
        "# Panics",
        "",
        "Panics if the runtime is unavailable.",
    ]

    # Act
    result = generate_docstrings.transform_doc(
        doc_lines,
        source_file="crates/network/src/python/http.rs",
        fn_name="py_request",
    )
    captured = capsys.readouterr()

    # Assert
    assert captured.err == ""
    assert "# Errors" in result
    assert "Returns an error when the request fails." in result
    assert "# Panics" not in result
    assert "Panics if the runtime is unavailable." not in result


def test_transform_doc_drops_errors_for_non_result_without_warning(capsys):
    # Arrange
    doc_lines = [
        "Does work.",
        "",
        "# Errors",
        "",
        "Returns an error when validation fails.",
    ]

    # Act
    result = generate_docstrings.transform_doc(
        doc_lines,
        source_file="crates/model/src/python/data/bet.rs",
        fn_name="py_probability_to_bet",
        strip_errors=True,
    )
    captured = capsys.readouterr()

    # Assert
    assert captured.err == ""
    assert result == ["Does work."]


def test_process_crate_preserves_errors_for_multiline_result_wrapper(tmp_path, monkeypatch, capsys):
    # Arrange
    monkeypatch.setattr(generate_docstrings, "ROOT", tmp_path)
    src_dir = tmp_path / "crates" / "network" / "src"
    python_dir = src_dir / "python"
    python_dir.mkdir(parents=True)

    (src_dir / "http.rs").write_text(
        """
pub struct HttpClient;

impl HttpClient {
    /// Sends an HTTP request.
    ///
    /// # Errors
    ///
    /// Returns an error when the request fails.
    ///
    /// # Panics
    ///
    /// Panics if the runtime is unavailable.
    pub fn request(&self) -> anyhow::Result<()> {
        todo!()
    }
}
""".strip(),
    )
    binding_path = python_dir / "http.rs"
    binding_path.write_text(
        """
#[pymethods]
impl HttpClient {
    #[pyo3(name = "request")]
    fn py_request<'py>(
        &self,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        todo!()
    }
}
""".strip(),
    )

    # Act
    updates = generate_docstrings.process_crate("network", src_dir)
    captured = capsys.readouterr()

    # Assert
    assert updates == 1
    assert captured.err == ""
    updated = binding_path.read_text()
    assert "/// # Errors" in updated
    assert "/// Returns an error when the request fails." in updated
    assert "/// # Panics" not in updated
    assert "/// Panics if the runtime is unavailable." not in updated


def test_collect_source_docs_attaches_doc_across_commented_attribute(tmp_path):
    # Arrange: the struct follows the commented attribute directly, so treating
    # the attribute as multi-line would swallow the struct and lose the doc
    source = """\
/// Calculates the thing.
///
/// More detail.
#[allow(dead_code)] // trailing comment
pub struct Foo {}

#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader")
)] // trailing comment on closing line
pub struct Bar {}
"""
    (tmp_path / "lib.rs").write_text(source)

    # Act
    docs = generate_docstrings.collect_source_docs(tmp_path)

    # Assert
    assert docs[(None, "Foo")] == ["Calculates the thing.", "", "More detail."]
    assert (None, "Bar") not in docs  # No doc comment on Bar

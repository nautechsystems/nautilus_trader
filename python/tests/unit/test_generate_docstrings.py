# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------
"""
Test generate docstrings behavior.
"""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

import pytest


def _load_generate_docstrings_module() -> object:
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
def test_attr_end_re_matches_closing_attribute_lines(line: object, expected: object) -> None:
    """
    Test attr end re matches closing attribute lines.
    """
    # Act
    result = generate_docstrings.ATTR_END_RE.search(line) is not None

    # Assert
    assert result is expected


def test_parse_pyo3_items_tolerates_attribute_with_trailing_comment() -> None:
    """
    Test parse pyo3 items tolerates attribute with trailing comment.
    """
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


def test_parse_pyo3_items_keeps_doc_after_parameter_attribute() -> None:
    """
    Test parse pyo3 items keeps the doc of a function after a parameter attribute.
    """
    # Arrange: the parameter attribute must not swallow the next function's doc comment
    lines = [
        "#[pymethods]",
        "impl PyCatalog {",
        "    pub fn query(",
        "        &mut self,",
        '        #[gen_stub(override_type(type_repr = "model.NautilusDataType"))] data_type: &Bound<',
        "            '_,",
        "            PyAny,",
        "        >,",
        "    ) -> PyResult<()> {",
        "        Ok(())",
        "    }",
        "}",
        "",
        "/// Reads a run.",
        "#[pyfunction]",
        "pub fn py_read_run() -> PyResult<()> {",
        "    Ok(())",
        "}",
    ]

    # Act
    items = generate_docstrings.parse_pyo3_items(lines)

    # Assert
    assert items == [
        {
            "fn_name": "query",
            "fn_line": 2,
            "fn_signature": (
                "pub fn query( &mut self, "
                '#[gen_stub(override_type(type_repr = "model.NautilusDataType"))] data_type: &Bound< '
                "'_, PyAny, >, ) -> PyResult<()> {"
            ),
            "impl_type": "PyCatalog",
            "is_constructor": False,
            "in_pymethods": True,
            "doc_start": None,
            "doc_end": None,
            "insert_line": 2,
        },
        {
            "fn_name": "py_read_run",
            "fn_line": 15,
            "fn_signature": "pub fn py_read_run() -> PyResult<()> {",
            "impl_type": None,
            "is_constructor": False,
            "in_pymethods": False,
            "doc_start": 13,
            "doc_end": 13,
            "insert_line": 13,
        },
    ]


def test_parse_pyo3_items_captures_multiline_result_signature() -> None:
    """
    Test parse pyo3 items captures multiline result signature.
    """
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
def test_rust_fn_returns_result_detects_return_type_only(
    signature: object,
    expected: object,
) -> None:
    """
    Test rust fn returns result detects return type only.
    """
    # Act
    result = generate_docstrings.rust_fn_returns_result(signature)

    # Assert
    assert result is expected


def test_transform_doc_drops_panics_without_warning(capsys: pytest.CaptureFixture[str]) -> None:
    """
    Test transform doc drops panics without warning.
    """
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


def test_transform_doc_drops_errors_for_non_result_without_warning(
    capsys: pytest.CaptureFixture[str],
) -> None:
    """
    Test transform doc drops errors for non result without warning.
    """
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


def test_process_crate_preserves_errors_for_multiline_result_wrapper(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """
    Test process crate preserves errors for multiline result wrapper.
    """
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
        encoding="utf-8",
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
        encoding="utf-8",
    )

    # Act
    updates = generate_docstrings.process_crate("network", src_dir)
    captured = capsys.readouterr()

    # Assert
    assert updates == 1
    assert captured.err == ""
    updated = binding_path.read_text(encoding="utf-8")
    assert "/// # Errors" in updated
    assert "/// Returns an error when the request fails." in updated
    assert "/// # Panics" not in updated
    assert "/// Panics if the runtime is unavailable." not in updated


def test_process_crate_only_reports_traversal_when_verbose(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """
    Test process crate only reports traversal when verbose.
    """
    # Act
    updates = generate_docstrings.process_crate("core", tmp_path)
    normal_output = capsys.readouterr()
    verbose_updates = generate_docstrings.process_crate("core", tmp_path, verbose=True)
    verbose_output = capsys.readouterr()

    # Assert
    assert updates == 0
    assert normal_output.out == ""
    assert normal_output.err == ""
    assert verbose_updates == 0
    assert verbose_output.out == (
        "Processing crate: core\n"
        "  Collected 0 source doc comments\n"
        "  No python/ directory, skipping\n"
    )
    assert verbose_output.err == ""


@pytest.mark.parametrize("name", ["quote", "trade", "bar"])
def test_process_crate_uses_indexed_cache_docs_for_shared_wrapper(
    name: str,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """
    Distinguish indexed shared accessors from native latest reads.
    """
    monkeypatch.setattr(generate_docstrings, "ROOT", tmp_path)
    src_dir = tmp_path / "crates" / "common" / "src"
    python_dir = src_dir / "python"
    python_dir.mkdir(parents=True)
    (src_dir / "cache.rs").write_text(
        f"""\
impl Cache {{
    /// Returns the latest {name}.
    pub fn {name}(&self) {{}}
    /// Returns the {name} at index, with index 0 most recent.
    pub fn {name}_at_index(&self, index: usize) {{}}
}}
""",
        encoding="utf-8",
    )
    binding_path = python_dir / "cache.rs"
    binding_path.write_text(
        f"""\
#[pymethods]
impl PyCache {{
    fn py_{name}(&self, index: usize) {{}}
}}
#[pymethods]
impl Cache {{
    fn py_{name}(&self) {{}}
}}
""",
        encoding="utf-8",
    )

    updates = generate_docstrings.process_crate("common", src_dir)
    updated = binding_path.read_text(encoding="utf-8")
    repeated_updates = generate_docstrings.process_crate("common", src_dir)

    assert updates == 2
    assert (
        updated
        == f"""\
#[pymethods]
impl PyCache {{
    /// Returns the {name} at index, with index 0 most recent.
    fn py_{name}(&self, index: usize) {{}}
}}
#[pymethods]
impl Cache {{
    /// Returns the latest {name}.
    fn py_{name}(&self) {{}}
}}
"""
    )
    assert repeated_updates == 0
    assert binding_path.read_text(encoding="utf-8") == updated


def test_process_crate_copies_cache_history_docs_without_overwriting_other_wrappers(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """
    Copy history docs to PyCache while preserving unrelated wrapper documentation.
    """
    monkeypatch.setattr(generate_docstrings, "ROOT", tmp_path)
    src_dir = tmp_path / "crates" / "common" / "src"
    python_dir = src_dir / "python"
    python_dir.mkdir(parents=True)
    (src_dir / "cache.rs").write_text(
        """\
impl Cache {
    /// Returns cached bars, newest first.
    ///
    /// Returns `None` if absent.
    pub fn bars(&self) {}
    /// Returns a native order reference.
    pub fn order(&self) {}
}
""",
        encoding="utf-8",
    )
    binding_path = python_dir / "cache.rs"
    binding_path.write_text(
        """\
#[pymethods]
impl PyCache {
    fn py_bars(&self) {}
    /// Returns a Python order snapshot.
    fn py_order(&self) {}
}
""",
        encoding="utf-8",
    )

    updates = generate_docstrings.process_crate("common", src_dir)
    updated = binding_path.read_text(encoding="utf-8")

    assert updates == 1
    assert (
        updated
        == """\
#[pymethods]
impl PyCache {
    /// Returns cached bars, newest first.
    ///
    /// Returns `None` if absent.
    fn py_bars(&self) {}
    /// Returns a Python order snapshot.
    fn py_order(&self) {}
}
"""
    )


def test_collect_source_docs_attaches_doc_across_commented_attribute(tmp_path: Path) -> None:
    """
    Test collect source docs attaches doc across commented attribute.
    """
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
    (tmp_path / "lib.rs").write_text(source, encoding="utf-8")

    # Act
    docs = generate_docstrings.collect_source_docs(tmp_path)

    # Assert
    assert docs[(None, "Foo")] == ["Calculates the thing.", "", "More detail."]
    assert (None, "Bar") not in docs  # No doc comment on Bar


def test_collect_source_docs_keeps_doc_after_parameter_attribute(tmp_path: Path) -> None:
    """
    Test collect source docs keeps the doc of an item after a parameter attribute.
    """
    # Arrange: the parameter attribute must not swallow the rest of the file
    source = """\
impl RetryManager {
    /// Runs the operation.
    pub fn run(
        #[builder(start_fn)] operation_name: &str,
    ) {
    }
}

/// Creates the default retry manager.
pub fn create_default_retry_manager() {}
"""
    (tmp_path / "lib.rs").write_text(source, encoding="utf-8")

    # Act
    docs = generate_docstrings.collect_source_docs(tmp_path)

    # Assert
    assert docs == {
        ("RetryManager", "run"): ["Runs the operation."],
        (None, "create_default_retry_manager"): ["Creates the default retry manager."],
    }


def test_process_crate_syncs_fields_forwarded_methods_and_enum_variants(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """
    Sync wrapper docs from their inner type, including later source changes.
    """
    monkeypatch.setattr(generate_docstrings, "ROOT", tmp_path)
    monkeypatch.setitem(generate_docstrings.TYPE_DOC_SOURCES, "PyFeed", "Feed")
    monkeypatch.setitem(generate_docstrings.TYPE_DOC_SOURCES, "PyStatus", "Status")
    src_dir = tmp_path / "crates" / "feed" / "src"
    python_dir = src_dir / "python"
    python_dir.mkdir(parents=True)
    source = src_dir / "lib.rs"
    source.write_text(
        """\
/// Provider response.
pub struct Feed {
    /// Maximum returned records.
    #[serde(default)]
    pub count: u64,
}

pub enum Status {
    /// Feed accepts requests.
    Ready,
}

impl Feed {
    /// Retrieves the latest response.
    pub fn latest(&self) {}
}
""",
        encoding="utf-8",
    )
    binding = python_dir / "feed.rs"
    binding.write_text(
        """\
pub struct PyFeed {
    inner: Feed,
}

pub enum PyStatus {
    /// Outdated variant description.
    Ready,
}

impl From<Status> for PyStatus {
    fn from(value: Status) -> Self { todo!() }
}

#[pymethods]
impl PyFeed {
    #[getter(count)]
    fn py_count(&self) -> u64 { self.inner.count }

    fn latest(&self) { self.inner.latest() }
}

fn latest() {}
""",
        encoding="utf-8",
    )

    updates = generate_docstrings.process_crate("feed", src_dir)
    first = binding.read_text(encoding="utf-8")
    repeated = generate_docstrings.process_crate("feed", src_dir)
    source.write_text(
        source.read_text(encoding="utf-8").replace("Maximum returned", "Maximum retained"),
        encoding="utf-8",
    )
    refreshed = generate_docstrings.process_crate("feed", src_dir)
    final = binding.read_text(encoding="utf-8")

    assert updates == 3
    assert repeated == 0
    assert refreshed == 1
    assert "    /// Maximum returned records.\n    #[getter(count)]" in first
    assert "    /// Feed accepts requests.\n    Ready," in first
    assert "    /// Retrieves the latest response.\n    fn latest(&self)" in first
    assert first.endswith("\nfn latest() {}\n")
    assert "Outdated variant description." not in first
    assert final == first.replace("Maximum returned", "Maximum retained")


def test_process_crate_reads_qualified_dependency_field_docs(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """
    Use the dependency field docs selected by an existing wrapper alias.
    """
    monkeypatch.setattr(generate_docstrings, "ROOT", tmp_path)
    monkeypatch.setattr(generate_docstrings, "CRATES_DIR", tmp_path / "crates")
    src_dir = tmp_path / "crates" / "finfeed" / "src"
    python_dir = src_dir / "python"
    python_dir.mkdir(parents=True)
    dependency = tmp_path / "crates" / "network" / "src"
    dependency.mkdir(parents=True)
    (dependency / "retry.rs").write_text(
        """\
pub struct RetryConfig {
    /// Retry attempts after the initial request.
    pub max_retries: u32,
}
""",
        encoding="utf-8",
    )
    binding = python_dir / "config.rs"
    binding.write_text(
        """\
#[pymethods]
impl FinFeedDataClientConfig {
    #[getter]
    fn max_retries(&self) -> u32 { self.http.retry.max_retries }
}
""",
        encoding="utf-8",
    )

    updates = generate_docstrings.process_crate("finfeed", src_dir)

    assert updates == 1
    assert binding.read_text(encoding="utf-8") == (
        "#[pymethods]\n"
        "impl FinFeedDataClientConfig {\n"
        "    /// Retry attempts after the initial request.\n"
        "    #[getter]\n"
        "    fn max_retries(&self) -> u32 { self.http.retry.max_retries }\n"
        "}\n"
    )


@pytest.mark.parametrize("name", ["retry_operation_timeout_ms", "retry_max_elapsed_ms"])
def test_finfeed_deadline_docs_preserve_required_timeouts(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    name: str,
) -> None:
    """
    Use required FinFeed deadline docs instead of generic optional timeouts.
    """
    root = Path(__file__).resolve().parents[3]
    monkeypatch.setattr(generate_docstrings, "ROOT", tmp_path)
    monkeypatch.setattr(generate_docstrings, "CRATES_DIR", tmp_path / "crates")
    for relative in ["adapters/finfeed/src/http/client.rs", "network/src/retry.rs"]:
        destination = tmp_path / "crates" / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(
            (root / "crates" / relative).read_text(encoding="utf-8"),
            encoding="utf-8",
        )
    src_dir = tmp_path / "crates" / "adapters" / "finfeed" / "src"
    python_dir = src_dir / "python"
    python_dir.mkdir()
    binding = python_dir / "config.rs"
    binding.write_text(
        "#[pymethods]\nimpl FinFeedDataClientConfig {\n"
        f"    #[getter]\n    fn {name}(&self) -> Option<u64> {{ None }}\n"
        "}\n",
        encoding="utf-8",
    )

    written = generate_docstrings.process_crate("finfeed", src_dir)
    result = binding.read_text(encoding="utf-8")

    assert written == 1
    assert (
        "/// Retry policy with required per-operation and total elapsed deadlines, in milliseconds."
        in result
    )
    assert (
        "/// Both include local admission waits; the total deadline also covers retries and backoff."
        in result
    )
    assert (
        "/// Deadlines must be positive and at most one day. Python `None` retains defaults;"
        in result
    )
    assert "/// a Rust `None` deadline fails client validation." in result
    assert "`None` disables" not in result


def test_process_crate_syncs_enum_only_module_without_writing_in_dry_run(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """
    Copy enum variant docs even when the module contains no methods.
    """
    monkeypatch.setattr(generate_docstrings, "ROOT", tmp_path)
    monkeypatch.setitem(generate_docstrings.TYPE_DOC_SOURCES, "PyStatus", "Status")
    python_dir = tmp_path / "python"
    python_dir.mkdir()
    (tmp_path / "lib.rs").write_text(
        "pub enum Status {\n    /// Unrecognized source label.\n    Unknown,\n}\n",
        encoding="utf-8",
    )
    binding = python_dir / "status.rs"
    original = "pub enum PyStatus {\n    Unknown,\n}\n"
    binding.write_text(original, encoding="utf-8")

    planned = generate_docstrings.process_crate("feed", tmp_path, dry_run=True)
    after_dry_run = binding.read_text(encoding="utf-8")
    written = generate_docstrings.process_crate("feed", tmp_path)

    assert planned == 1
    assert after_dry_run == original
    assert written == 1
    assert binding.read_text(encoding="utf-8") == (
        "pub enum PyStatus {\n    /// Unrecognized source label.\n    Unknown,\n}\n"
    )


def test_process_crate_preserves_predicate_method_contract(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """
    Keep predicate method docs distinct from optional-field presence getters.
    """
    monkeypatch.setattr(generate_docstrings, "ROOT", tmp_path)
    python_dir = tmp_path / "python"
    python_dir.mkdir()
    (tmp_path / "lib.rs").write_text(
        """\
impl Cache {
    /// Returns whether quotes exist for the requested instrument.
    pub fn has_quotes(&self, instrument: InstrumentId) -> bool { todo!() }
}

pub struct FinFeedDataClientConfig {
    /// Optional explicit credential.
    pub api_key: Option<SecretString>,
}
""",
        encoding="utf-8",
    )
    binding = python_dir / "cache.rs"
    binding.write_text(
        """\
#[pymethods]
impl Cache {
    fn py_has_quotes(&self, instrument: InstrumentId) -> bool { todo!() }
}

#[pymethods]
impl FinFeedDataClientConfig {
    #[getter]
    fn has_api_key(&self) -> bool { self.api_key.is_some() }
}
""",
        encoding="utf-8",
    )

    written = generate_docstrings.process_crate("feed", tmp_path)
    repeated = generate_docstrings.process_crate("feed", tmp_path)
    result = binding.read_text(encoding="utf-8")

    assert written == 2
    assert repeated == 0
    assert "    /// Returns whether quotes exist for the requested instrument.\n" in result
    assert "    /// Returns whether `api_key` is set.\n    #[getter]\n" in result

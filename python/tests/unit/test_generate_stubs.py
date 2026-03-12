from __future__ import annotations

import importlib.util
from pathlib import Path
import sys


def _load_generate_stubs_module():
    module_path = Path(__file__).resolve().parents[2] / "generate_stubs.py"
    module_name = "generate_stubs_module"
    spec = importlib.util.spec_from_file_location(module_name, module_path)
    module = importlib.util.module_from_spec(spec)
    assert spec is not None
    assert spec.loader is not None
    sys.modules[module_name] = module
    spec.loader.exec_module(module)
    return module


generate_stubs = _load_generate_stubs_module()


def test_collect_rust_class_fixups_reads_pymethods_and_identifier_macros(tmp_path):
    # Arrange
    rust_file = tmp_path / "crates" / "model" / "src" / "python" / "sample.rs"
    rust_file.parent.mkdir(parents=True)
    rust_file.write_text(
        """
#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl Sample {
    #[getter]
    #[pyo3(name = "value")]
    fn py_value(&self) -> String {
        todo!()
    }

    #[staticmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(value: &str) -> Self {
        todo!()
    }
}

#[pymethods]
impl Sample {
    #[staticmethod]
    #[pyo3(name = "from_json")]
    fn py_from_json(data: &[u8]) -> PyResult<Self> {
        todo!()
    }
}

identifier_for_python!(crate::identifiers::AccountId);
""".strip(),
    )

    # Act
    fixups = generate_stubs.collect_rust_class_fixups(tmp_path)

    # Assert
    assert fixups["Sample"].getters == {"value"}
    assert fixups["Sample"].staticmethods == {"from_json", "from_str"}
    assert (
        fixups["Sample"].injected_staticmethods["from_json"]
        == "    @staticmethod\n    def from_json(data: typing.Any) -> Sample: ..."
    )
    assert fixups["AccountId"].getters == {"value"}
    assert fixups["AccountId"].staticmethods == {"_safe_constructor", "from_str"}


def test_collect_rust_class_fixups_keeps_fallback_name_when_pyo3_name_is_ignored(tmp_path):
    # Arrange
    rust_file = tmp_path / "crates" / "model" / "src" / "python" / "sample.rs"
    rust_file.parent.mkdir(parents=True)
    rust_file.write_text(
        """
#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl Currency {
    #[staticmethod]
    #[pyo3(name = "is_commodity_backed")]
    fn py_is_commodidity_backed(code: &str) -> PyResult<bool> {
        todo!()
    }
}
""".strip(),
    )

    # Act
    fixups = generate_stubs.collect_rust_class_fixups(tmp_path)

    # Assert
    assert fixups["Currency"].staticmethods == {"is_commodity_backed", "is_commodidity_backed"}
    assert fixups["Currency"].renames == {"is_commodidity_backed": "is_commodity_backed"}


def test_collect_rust_class_fixups_detects_classmethods(tmp_path):
    # Arrange
    rust_file = tmp_path / "crates" / "model" / "src" / "python" / "sample.rs"
    rust_file.parent.mkdir(parents=True)
    rust_file.write_text(
        """
#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PriceType {
    #[classmethod]
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        todo!()
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        todo!()
    }
}
""".strip(),
    )

    # Act
    fixups = generate_stubs.collect_rust_class_fixups(tmp_path)

    # Assert
    assert fixups["PriceType"].classmethods == {"variants", "from_str"}
    assert fixups["PriceType"].staticmethods == set()


def test_collect_rust_class_fixups_preserves_attrs_across_doc_comments(tmp_path):
    # Arrange
    rust_file = tmp_path / "crates" / "model" / "src" / "python" / "sample.rs"
    rust_file.parent.mkdir(parents=True)
    rust_file.write_text(
        """
#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl AccountState {
    #[staticmethod]
    /// Constructs an [`AccountState`] from a Python dict.
    ///
    /// # Errors
    ///
    /// Returns a `PyErr` if conversion fails.
    #[pyo3(name = "from_dict")]
    pub fn py_from_dict(values: &Bound<'_, PyDict>) -> PyResult<Self> {
        todo!()
    }
}
""".strip(),
    )

    # Act
    fixups = generate_stubs.collect_rust_class_fixups(tmp_path)

    # Assert
    assert "from_dict" in fixups["AccountState"].staticmethods


def test_collect_rust_class_fixups_handles_multiline_attributes(tmp_path):
    # Arrange
    rust_file = tmp_path / "crates" / "core" / "src" / "python" / "sample.rs"
    rust_file.parent.mkdir(parents=True)
    rust_file.write_text(
        """
#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl UUID4 {
    #[staticmethod]
    #[allow(
        clippy::unnecessary_wraps,
        reason = "Python FFI requires Result return type"
    )]
    fn _safe_constructor() -> PyResult<Self> {
        todo!()
    }
}
""".strip(),
    )

    # Act
    fixups = generate_stubs.collect_rust_class_fixups(tmp_path)

    # Assert
    assert fixups["UUID4"].staticmethods == {"_safe_constructor"}


def test_apply_rust_class_fixups_restores_properties_and_staticmethods():
    # Arrange
    content = """
@typing.final
class Sample:
    def value(self) -> builtins.str: ...
    def from_str(self, value: builtins.str) -> Sample: ...
    def get_metadata(
        self, value: builtins.str
    ) -> dict: ...
""".strip()
    fixups = {
        "Sample": generate_stubs.ClassMethodFixup(
            getters={"value"},
            staticmethods={"from_str", "get_metadata"},
        ),
    }

    # Act
    updated = generate_stubs.apply_rust_class_fixups(content, fixups)

    # Assert
    assert "    @property\n    def value(self) -> builtins.str: ..." in updated
    assert "    @staticmethod\n    def from_str(value: builtins.str) -> Sample: ..." in updated
    assert "    @staticmethod\n    def get_metadata(\n        value: builtins.str" in updated
    assert "def from_str(self, value: builtins.str)" not in updated
    assert "def get_metadata(\n        self, value: builtins.str" not in updated


def test_apply_rust_class_fixups_injects_missing_deserializers():
    # Arrange
    content = """
@typing.final
class Sample:
    def to_json_bytes(self) -> typing.Any: ...

@typing.final
class Other:
    pass
""".strip()
    fixups = {
        "Sample": generate_stubs.ClassMethodFixup(
            injected_staticmethods={
                "from_json": "    @staticmethod\n    def from_json(data: typing.Any) -> Sample: ...",
                "from_msgpack": "    @staticmethod\n    def from_msgpack(data: typing.Any) -> Sample: ...",
            },
        ),
    }

    # Act
    updated = generate_stubs.apply_rust_class_fixups(content, fixups)

    # Assert
    assert "    @staticmethod\n    def from_json(data: typing.Any) -> Sample: ..." in updated
    assert "    @staticmethod\n    def from_msgpack(data: typing.Any) -> Sample: ..." in updated
    assert (
        "def from_msgpack(data: typing.Any) -> Sample: ...\n\n@typing.final\nclass Other:"
        in updated
    )
    assert updated.index("def from_msgpack(data: typing.Any) -> Sample: ...") < updated.index(
        "@typing.final\nclass Other:",
    )


def test_apply_rust_class_fixups_suppresses_implementation_detail_methods():
    # Arrange
    content = """
@typing.final
class Sample:
    def __init__(self, value: str) -> None: ...
    def _safe_constructor(self) -> Sample: ...
    def __richcmp__(self, other: Sample, op: int) -> typing.Any: ...
    def __hash__(self) -> int: ...
    def value(self) -> str: ...
""".strip()
    fixups = {
        "Sample": generate_stubs.ClassMethodFixup(
            getters={"value"},
        ),
    }

    # Act
    updated = generate_stubs.apply_rust_class_fixups(content, fixups)

    # Assert
    assert "_safe_constructor" not in updated
    assert "__richcmp__" not in updated
    assert "__init__" in updated
    assert "__hash__" in updated
    assert "@property\n    def value" in updated


def test_apply_rust_class_fixups_adds_classmethod_decorator():
    # Arrange
    content = """
class PriceType(Enum):
    Bid = ...
    Ask = ...
    def variants(self) -> EnumIterator: ...
    def from_str(self, data: str) -> PriceType: ...
""".strip()
    fixups = {
        "PriceType": generate_stubs.ClassMethodFixup(
            classmethods={"variants", "from_str"},
        ),
    }

    # Act
    updated = generate_stubs.apply_rust_class_fixups(content, fixups)

    # Assert
    assert "    @classmethod\n    def variants(cls) -> EnumIterator: ..." in updated
    assert "    @classmethod\n    def from_str(cls, data: str) -> PriceType: ..." in updated
    assert "def variants(self)" not in updated
    assert "def from_str(self," not in updated


def test_apply_rust_class_fixups_renames_methods():
    # Arrange
    content = """
@typing.final
class Currency:
    @staticmethod
    def is_commodidity_backed(code: str) -> bool: ...
    @staticmethod
    def arbitrum_chain() -> Chain: ...
""".strip()
    fixups = {
        "Currency": generate_stubs.ClassMethodFixup(
            staticmethods={
                "is_commodity_backed",
                "is_commodidity_backed",
                "arbitrum_chain",
                "ARBITRUM",
            },
            renames={
                "is_commodidity_backed": "is_commodity_backed",
                "arbitrum_chain": "ARBITRUM",
            },
        ),
    }

    # Act
    updated = generate_stubs.apply_rust_class_fixups(content, fixups)

    # Assert
    assert "def is_commodity_backed(code: str) -> bool: ..." in updated
    assert "is_commodidity_backed" not in updated
    assert "def ARBITRUM() -> Chain: ..." in updated
    assert "arbitrum_chain" not in updated


def test_normalize_stub_content_strips_builtin_type_qualifiers():
    # Arrange
    content = """
import builtins
import typing

def parse(values: builtins.list[builtins.int]) -> builtins.dict[builtins.str, builtins.bool]: ...
""".strip()

    # Act
    updated = generate_stubs.normalize_stub_content(content)

    # Assert
    assert "import builtins" not in updated
    assert "def parse(values: list[int]) -> dict[str, bool]: ..." in updated
    assert "builtins." not in updated


def test_normalize_stub_content_preserves_builtins_import_when_still_needed():
    # Arrange
    content = """
import builtins

def parse_error() -> builtins.Exception: ...
""".strip()

    # Act
    updated = generate_stubs.normalize_stub_content(content)

    # Assert
    assert "import builtins" in updated
    assert "builtins.Exception" in updated

from __future__ import annotations

import ast
import importlib
import inspect
import keyword
import os
import re
import sys
from pathlib import Path

import pytest


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


@pytest.mark.parametrize(
    ("platform", "shared", "libdir", "existing", "expected_var", "expected_value"),
    [
        ("linux", 1, "/uv/lib", None, "LD_LIBRARY_PATH", "/uv/lib"),
        ("linux", 1, "/uv/lib", "/existing", "LD_LIBRARY_PATH", f"/uv/lib{os.pathsep}/existing"),
        ("darwin", 1, "/uv/lib", None, "DYLD_LIBRARY_PATH", "/uv/lib"),
        ("win32", 1, "/uv/lib", None, None, None),
        ("linux", 0, "/uv/lib", None, None, None),
        ("linux", 1, None, None, None, None),
    ],
)
def test_python_libdir_env_sets_loader_path(
    monkeypatch,
    platform,
    shared,
    libdir,
    existing,
    expected_var,
    expected_value,
):
    # Arrange
    monkeypatch.setattr(generate_stubs.sys, "platform", platform)
    monkeypatch.setattr(
        generate_stubs.sysconfig,
        "get_config_var",
        {"Py_ENABLE_SHARED": shared, "LIBDIR": libdir}.get,
    )
    monkeypatch.delenv("LD_LIBRARY_PATH", raising=False)
    monkeypatch.delenv("DYLD_LIBRARY_PATH", raising=False)
    if existing is not None:
        monkeypatch.setenv(expected_var, existing)

    # Act
    env = generate_stubs.python_libdir_env()

    # Assert
    if expected_var is None:
        assert "LD_LIBRARY_PATH" not in env
        assert "DYLD_LIBRARY_PATH" not in env
    else:
        assert env[expected_var] == expected_value


def test_python_libdir_env_does_not_mutate_os_environ(monkeypatch):
    # Arrange
    monkeypatch.setattr(generate_stubs.sys, "platform", "linux")
    monkeypatch.setattr(
        generate_stubs.sysconfig,
        "get_config_var",
        {"Py_ENABLE_SHARED": 1, "LIBDIR": "/uv/lib"}.get,
    )
    monkeypatch.delenv("LD_LIBRARY_PATH", raising=False)

    # Act
    env = generate_stubs.python_libdir_env()

    # Assert
    assert env["LD_LIBRARY_PATH"] == "/uv/lib"
    assert "LD_LIBRARY_PATH" not in os.environ


def test_write_config_stub_uses_runtime_exports(tmp_path):
    # Arrange
    runtime_path = tmp_path / "config" / "__init__.py"
    runtime_path.parent.mkdir()
    runtime_path.write_text(
        """
from __future__ import annotations

from nautilus_trader.analysis import TearsheetConfig
from nautilus_trader.common import CacheConfig

__all__ = [
    "CacheConfig",
    "TearsheetConfig",
]
""".lstrip(),
    )

    # Act
    generate_stubs.write_config_stub(tmp_path)

    # Assert
    stub = runtime_path.with_suffix(".pyi").read_text()
    assert "from nautilus_trader.common import CacheConfig as CacheConfig" in stub
    assert "from nautilus_trader.analysis import TearsheetConfig as TearsheetConfig" in stub
    assert ast.literal_eval(
        next(
            node.value
            for node in ast.parse(stub).body
            if isinstance(node, ast.Assign)
            and any(
                isinstance(target, ast.Name) and target.id == "__all__" for target in node.targets
            )
        ),
    ) == ["CacheConfig", "TearsheetConfig"]


def test_write_config_stub_rejects_export_drift(tmp_path):
    # Arrange
    runtime_path = tmp_path / "config" / "__init__.py"
    runtime_path.parent.mkdir()
    runtime_path.write_text(
        """
from nautilus_trader.common import CacheConfig

__all__ = ["TearsheetConfig"]
""".lstrip(),
    )

    # Act
    with pytest.raises(
        ValueError,
        match=r"^Config facade imports and __all__ differ",
    ) as exc_info:
        generate_stubs.write_config_stub(tmp_path)

    # Assert
    assert str(exc_info.value) == (
        "Config facade imports and __all__ differ: missing imports ['TearsheetConfig'], "
        "unexported imports ['CacheConfig']"
    )


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


def test_collect_rust_class_fixups_renames_get_prefixed_getter(tmp_path):
    # Arrange
    rust_file = tmp_path / "crates" / "adapter" / "src" / "python" / "config.rs"
    rust_file.parent.mkdir(parents=True)
    rust_file.write_text(
        """
#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl ClientConfig {
    #[getter]
    fn get_ws_url(&self) -> Option<String> {
        todo!()
    }
}
""".strip(),
    )

    # Act
    fixups = generate_stubs.collect_rust_class_fixups(tmp_path)

    # Assert
    assert fixups["ClientConfig"].getters == {"get_ws_url", "ws_url"}
    assert fixups["ClientConfig"].renames == {"get_ws_url": "ws_url"}


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


def test_signature_defaults_handle_lifetime_generic_methods(tmp_path):
    # Arrange
    rust_file = tmp_path / "crates" / "adapters" / "hyperliquid" / "src" / "python" / "http.rs"
    rust_file.parent.mkdir(parents=True)
    rust_file.write_text(
        """
#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl HyperliquidHttpClient {
    #[pyo3(name = "load_instrument_definitions", signature = (include_spot=true, include_perps=true, include_perps_hip3=false, include_outcomes=false))]
    fn py_load_instrument_definitions<'py>(
        &self,
        py: Python<'py>,
        include_spot: bool,
        include_perps: bool,
        include_perps_hip3: bool,
        include_outcomes: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        todo!()
    }
}
""".strip(),
    )
    content = """
class HyperliquidHttpClient:
    def load_instrument_definitions(
        self,
        include_spot: bool,
        include_perps: bool,
        include_perps_hip3: bool,
        include_outcomes: bool,
    ) -> typing.Any: ...
""".strip()

    # Act
    fixups = generate_stubs.collect_rust_class_fixups(tmp_path)
    updated = generate_stubs.apply_signature_defaults(content, fixups)

    # Assert
    assert "include_spot: bool = True" in updated
    assert "include_perps: bool = True" in updated
    assert "include_perps_hip3: bool = False" in updated
    assert "include_outcomes: bool = False" in updated


def test_signature_defaults_ignore_var_kwargs_when_filtering_safe_defaults(tmp_path):
    rust_file = tmp_path / "crates" / "common" / "src" / "python" / "actor.rs"
    rust_file.parent.mkdir(parents=True)
    rust_file.write_text(
        """
#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl DataActorConfig {
    #[new]
    #[pyo3(signature = (actor_id=None, log_events=true, log_commands=true, **_kwargs))]
    fn py_new(
        actor_id: Option<ActorId>,
        log_events: bool,
        log_commands: bool,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> Self {
        todo!()
    }
}
""".strip(),
    )
    content = """
class DataActorConfig:
    def __init__(
        self,
        actor_id: model.ActorId | None,
        log_events: bool,
        log_commands: bool,
        _kwargs: dict | None = ...,
    ) -> None: ...
""".strip()

    fixups = generate_stubs.collect_rust_class_fixups(tmp_path)
    updated = generate_stubs.apply_signature_defaults(content, fixups)

    assert "actor_id: model.ActorId | None = None" in updated
    assert "log_events: bool = True" in updated
    assert "log_commands: bool = True" in updated
    assert "_kwargs: dict | None = ..." in updated


def test_signature_defaults_replace_stale_stub_defaults(tmp_path):
    rust_file = tmp_path / "crates" / "trading" / "src" / "python" / "strategy.rs"
    rust_file.parent.mkdir(parents=True)
    rust_file.write_text(
        """
#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl StrategyConfig {
    #[new]
    #[pyo3(signature = (log_events=false))]
    fn py_new(log_events: bool) -> Self {
        todo!()
    }
}
""".strip(),
    )
    content = """
class StrategyConfig:
    def __init__(self, log_events: bool = True) -> None: ...
""".strip()

    fixups = generate_stubs.collect_rust_class_fixups(tmp_path)
    updated = generate_stubs.apply_signature_defaults(content, fixups)

    assert "log_events: bool = False" in updated


def test_collect_rust_class_fixups_reads_custom_data_stub_module(tmp_path):
    # Arrange
    rust_file = tmp_path / "crates" / "adapters" / "hyperliquid" / "src" / "data_types.rs"
    rust_file.parent.mkdir(parents=True)
    rust_file.write_text(
        """
#[custom_data(pyo3, no_arrow, stub_module = "nautilus_trader.adapters.hyperliquid")]
pub struct HyperliquidAllMids {
    #[custom_data_field(json)]
    pub mids: HashMap<InstrumentId, Price>,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}
""".strip(),
    )

    # Act
    fixups = generate_stubs.collect_rust_class_fixups(tmp_path)

    # Assert
    assert fixups["HyperliquidAllMids"].getters == {"mids", "ts_event", "ts_init"}
    assert fixups["HyperliquidAllMids"].classmethods == {"from_json"}


def test_collect_rust_class_fixups_detects_cfg_attr_wrapped_custom_data(tmp_path):
    # Arrange: mirrors the multi-line cfg_attr form used in
    # crates/adapters/hyperliquid/src/data_types.rs
    rust_file = tmp_path / "crates" / "adapters" / "hyperliquid" / "src" / "data_types.rs"
    rust_file.parent.mkdir(parents=True)
    rust_file.write_text(
        """
#[cfg_attr(
    feature = "arrow",
    custom_data(pyo3, stub_module = "nautilus_trader.adapters.hyperliquid")
)]
#[cfg_attr(
    not(feature = "arrow"),
    custom_data(pyo3, no_arrow, stub_module = "nautilus_trader.adapters.hyperliquid")
)]
pub struct HyperliquidAllMids {
    #[custom_data_field(json)]
    pub mids: HashMap<InstrumentId, Price>,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}
""".strip(),
    )

    # Act
    fixups = generate_stubs.collect_rust_class_fixups(tmp_path)

    # Assert
    assert fixups["HyperliquidAllMids"].getters == {"mids", "ts_event", "ts_init"}
    assert fixups["HyperliquidAllMids"].classmethods == {"from_json"}


def test_collect_rust_class_fixups_ignores_custom_data_without_stub_module(tmp_path):
    # Arrange: DeribitVolatilityIndex pattern, cfg_attr-wrapped custom_data
    # with no stub_module must not register stub fixups.
    rust_file = tmp_path / "crates" / "adapters" / "deribit" / "src" / "data_types.rs"
    rust_file.parent.mkdir(parents=True)
    rust_file.write_text(
        """
#[cfg_attr(feature = "arrow", custom_data(pyo3))]
#[cfg_attr(not(feature = "arrow"), custom_data(pyo3, no_arrow))]
pub struct DeribitVolatilityIndex {
    pub index_name: String,
    pub volatility: f64,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}
""".strip(),
    )

    # Act
    fixups = generate_stubs.collect_rust_class_fixups(tmp_path)

    # Assert
    assert "DeribitVolatilityIndex" not in fixups


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


def test_collect_rust_class_fixups_handles_multiline_attributes_before_impl(tmp_path):
    # Arrange
    rust_file = tmp_path / "crates" / "trading" / "src" / "python" / "sample.rs"
    rust_file.parent.mkdir(parents=True)
    rust_file.write_text(
        """
#[pyo3::pyclass(name = "Strategy")]
struct PyStrategy {}

#[pyo3::pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[expect(
    clippy::large_types_passed_by_value,
    clippy::unused_self,
    reason = "default PyO3 callbacks must remain instance methods"
)]
impl PyStrategy {
    #[getter]
    #[pyo3(name = "trader_id")]
    fn py_trader_id(&self) -> Option<TraderId> {
        todo!()
    }
}
""".strip(),
    )

    # Act
    fixups = generate_stubs.collect_rust_class_fixups(tmp_path)
    updated = generate_stubs.apply_rust_class_fixups(
        """
@typing.final
class PyStrategy:
    def trader_id(self) -> model.TraderId | None: ...
""".strip(),
        fixups,
    )
    updated = generate_stubs.rename_stub_classes(updated, fixups)

    # Assert
    assert fixups["PyStrategy"].python_name == "Strategy"
    assert fixups["PyStrategy"].getters == {"trader_id"}
    assert "class Strategy:" in updated
    assert "    @property\n    def trader_id(self) -> model.TraderId | None: ..." in updated


def test_collect_rust_class_fixups_detects_cfg_attr_subclass_pyclass(tmp_path):
    # Arrange
    rust_file = tmp_path / "crates" / "trading" / "src" / "strategy" / "config.rs"
    rust_file.parent.mkdir(parents=True)
    rust_file.write_text(
        """
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.trading",
        subclass,
        from_py_object
    )
)]
pub struct StrategyConfig {}
""".strip(),
    )

    # Act
    fixups = generate_stubs.collect_rust_class_fixups(tmp_path)

    # Assert
    assert fixups["StrategyConfig"].subclass is True


def test_collect_rust_class_fixups_ignores_subclass_in_pyclass_string_values(tmp_path):
    # Arrange
    rust_file = tmp_path / "crates" / "test" / "src" / "config.rs"
    rust_file.parent.mkdir(parents=True)
    rust_file.write_text(
        """
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.adapters.subclass",
        name = "SubclassNamedConfig",
        from_py_object
    )
)]
pub struct RustConfig {}
""".strip(),
    )

    # Act
    fixups = generate_stubs.collect_rust_class_fixups(tmp_path)

    # Assert
    assert fixups["RustConfig"].python_name == "SubclassNamedConfig"
    assert fixups["RustConfig"].subclass is False


def test_remove_final_from_subclassable_classes():
    # Arrange
    content = """
@typing.final
class StrategyConfig:
    pass

@typing.final
class NonSubclassable:
    pass
""".strip()
    fixups = {
        "StrategyConfig": generate_stubs.ClassMethodFixup(subclass=True),
        "NonSubclassable": generate_stubs.ClassMethodFixup(subclass=False),
    }

    # Act
    updated = generate_stubs.remove_final_from_subclassable_classes(content, fixups)

    # Assert
    assert "@typing.final\nclass StrategyConfig:" not in updated
    assert "class StrategyConfig:" in updated
    assert "@typing.final\nclass NonSubclassable:" in updated


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


def test_apply_rust_class_fixups_rewrites_setters_as_property_setters():
    # Arrange
    content = """
class DataActorConfig:
    @property
    def actor_id(self) -> model.ActorId | None: ...
    def set_actor_id(self, actor_id: model.ActorId | None = ...) -> None: ...
""".strip()
    fixups = {
        "DataActorConfig": generate_stubs.ClassMethodFixup(
            getters={"actor_id"},
            setters={"set_actor_id": "actor_id"},
        ),
    }

    # Act
    updated = generate_stubs.apply_rust_class_fixups(content, fixups)

    # Assert
    assert "    @actor_id.setter\n    def actor_id(" in updated
    assert "actor_id: model.ActorId | None) -> None: ..." in updated
    assert "def set_actor_id" not in updated


def test_add_optional_defaults_skips_property_setters():
    # Arrange
    content = """
class DataActorConfig:
    def __init__(self, actor_id: model.ActorId | None) -> None: ...
    @property
    def actor_id(self) -> model.ActorId | None: ...
    @actor_id.setter
    def actor_id(self, actor_id: model.ActorId | None) -> None: ...
""".strip()

    # Act
    updated = generate_stubs.add_optional_defaults(content)

    # Assert
    assert "def __init__(self, actor_id: model.ActorId | None = ...) -> None: ..." in updated
    assert "def actor_id(self, actor_id: model.ActorId | None) -> None: ..." in updated


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


def test_apply_rust_class_fixups_drops_extra_classmethod_cls_param():
    # Arrange
    content = """
@typing.final
class HyperliquidAllMids:
    def from_json(self, _cls: type, data: typing.Any) -> typing.Any: ...
""".strip()
    fixups = {
        "HyperliquidAllMids": generate_stubs.ClassMethodFixup(
            classmethods={"from_json"},
        ),
    }

    # Act
    updated = generate_stubs.apply_rust_class_fixups(content, fixups)

    # Assert
    assert "    @classmethod\n    def from_json(cls, data: typing.Any)" in updated
    assert "_cls" not in updated


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


@pytest.mark.parametrize(
    ("input_name", "expected"),
    [
        ("Cash", "CASH"),
        ("Margin", "MARGIN"),
        ("StableSwap", "STABLE_SWAP"),
        ("WeightedPool", "WEIGHTED_POOL"),
        ("CLAMEnhanced", "CLAM_ENHANCED"),
        ("FluidDEX", "FLUID_DEX"),
        ("UniswapV2", "UNISWAP_V2"),
        ("PancakeSwapV3", "PANCAKE_SWAP_V3"),
        ("AerodromeSlipstream", "AERODROME_SLIPSTREAM"),
        ("NoOrderSide", "NO_ORDER_SIDE"),
        ("CPAMM", "CPAMM"),
        ("CLAMM", "CLAMM"),
        ("L1_MBP", "L1_MBP"),
        ("L2_MBP", "L2_MBP"),
        ("Level1", "LEVEL1"),
        ("BaseX", "BASE_X"),
        ("A", "A"),
        ("", ""),
        ("CASH", "CASH"),
        ("NO_ORDER_SIDE", "NO_ORDER_SIDE"),
    ],
)
def test_to_screaming_snake_case(input_name, expected):
    assert generate_stubs.to_screaming_snake_case(input_name) == expected


def test_rename_enum_variants_transforms_renamed_enums():
    # Arrange
    content = """
class AccountType(Enum):
    Cash = ...
    Margin = ...
    Betting = ...

    def __init__(self, value: typing.Any) -> None: ...
    @property
    def name(self) -> str: ...

class OtherClass:
    def method(self) -> None: ...
""".strip()
    renamed_enums = {"AccountType"}

    # Act
    updated = generate_stubs.rename_enum_variants(content, renamed_enums)

    # Assert
    assert "    CASH = ..." in updated
    assert "    MARGIN = ..." in updated
    assert "    BETTING = ..." in updated
    assert "    Cash" not in updated
    assert "def __init__" in updated
    assert "class OtherClass:" in updated


def test_rename_enum_variants_skips_non_renamed_enums():
    # Arrange
    content = """
class BookType(Enum):
    L1_MBP = ...
    L2_MBP = ...
""".strip()
    renamed_enums = set()

    # Act
    updated = generate_stubs.rename_enum_variants(content, renamed_enums)

    # Assert
    assert updated == content


def test_rename_enum_variants_handles_enum_dot_enum_base():
    # Arrange
    content = """
class HyperliquidProductType(enum.Enum):
    Perp = ...
    Spot = ...

    def __init__(self, value: typing.Any) -> None: ...
""".strip()
    renamed_enums = {"HyperliquidProductType"}

    # Act
    updated = generate_stubs.rename_enum_variants(content, renamed_enums)

    # Assert
    assert "    PERP = ..." in updated
    assert "    SPOT = ..." in updated


def test_rename_enum_variants_handles_multi_word_variants():
    # Arrange
    content = """
class DexType(Enum):
    AerodromeSlipstream = ...
    UniswapV2 = ...
    PancakeSwapV3 = ...
    FluidDEX = ...
    CLAMEnhanced = ...
""".strip()
    renamed_enums = {"DexType"}

    # Act
    updated = generate_stubs.rename_enum_variants(content, renamed_enums)

    # Assert
    assert "    AERODROME_SLIPSTREAM = ..." in updated
    assert "    UNISWAP_V2 = ..." in updated
    assert "    PANCAKE_SWAP_V3 = ..." in updated
    assert "    FLUID_DEX = ..." in updated
    assert "    CLAM_ENHANCED = ..." in updated


def test_rename_enum_variants_uses_source_variants_for_digit_boundaries():
    # Arrange
    content = """
class Blockchain(Enum):
    HarmonySharD0 = ...
    MetalL2 = ...
""".strip()
    renamed_enums = {"Blockchain"}
    renamed_enum_variants = {"Blockchain": ["HarmonyShard0", "Metall2"]}

    # Act
    updated = generate_stubs.rename_enum_variants(
        content,
        renamed_enums,
        renamed_enum_variants,
    )

    # Assert
    assert "    HARMONY_SHARD0 = ..." in updated
    assert "    METALL2 = ..." in updated
    assert "    HARMONY_SHAR_D0" not in updated
    assert "    METAL_L2" not in updated


def test_collect_renamed_enums_detects_rename_all(tmp_path):
    # Arrange
    rust_file = tmp_path / "crates" / "model" / "src" / "enums.rs"
    rust_file.parent.mkdir(parents=True)
    rust_file.write_text(
        """
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
pub enum AccountType {
    Cash = 1,
    Margin = 2,
}

#[cfg_attr(
    feature = "python",
    pyo3::pyclass(frozen, eq, eq_int)
)]
pub enum PlainEnum {
    Foo = 1,
}

pub struct NotAnEnum {
    field: u8,
}
""".strip(),
    )

    # Act
    result = generate_stubs.collect_renamed_enums(tmp_path)

    # Assert
    assert "AccountType" in result
    assert "PlainEnum" not in result
    assert "NotAnEnum" not in result


def test_collect_module_constants_detects_m_add(tmp_path):
    # Arrange
    mod_rs = tmp_path / "crates" / "core" / "src" / "python" / "mod.rs"
    mod_rs.parent.mkdir(parents=True)
    mod_rs.write_text(
        """
#[pymodule]
pub fn core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add(stringify!(MY_VERSION), MY_VERSION)?;
    m.add("MY_CONSTANT", crate::MY_CONSTANT)?;
    m.add("MyException", m.py().get_type::<MyException>())?;
    Ok(())
}
""".strip(),
    )

    const_rs = tmp_path / "crates" / "core" / "src" / "consts.rs"
    const_rs.parent.mkdir(parents=True, exist_ok=True)
    const_rs.write_text(
        """
pub static MY_VERSION: &str = "1.0.0";
pub const MY_CONSTANT: u64 = 42;
""".strip(),
    )

    # Act
    result = generate_stubs.collect_module_constants(tmp_path)

    # Assert
    assert "core" in result
    consts = result["core"]
    names = [c.name for c in consts]
    assert "MY_VERSION" in names
    assert "MY_CONSTANT" in names
    assert "MyException" not in names
    assert consts[names.index("MY_VERSION")].python_type == "str"
    assert consts[names.index("MY_CONSTANT")].python_type == "int"


def test_collect_module_constants_uses_adapter_package_path(tmp_path):
    # Arrange
    mod_rs = tmp_path / "crates" / "adapters" / "polymarket" / "src" / "python" / "mod.rs"
    mod_rs.parent.mkdir(parents=True)
    mod_rs.write_text(
        """
#[pymodule]
pub fn polymarket(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add(stringify!(POLYMARKET), POLYMARKET)?;
    Ok(())
}
""".strip(),
    )

    const_rs = tmp_path / "crates" / "adapters" / "polymarket" / "src" / "common" / "consts.rs"
    const_rs.parent.mkdir(parents=True, exist_ok=True)
    const_rs.write_text(
        """
pub const POLYMARKET: &str = "POLYMARKET";
""".strip(),
    )

    # Act
    result = generate_stubs.collect_module_constants(tmp_path)

    # Assert
    assert "adapters.polymarket" in result
    assert "polymarket" not in result


def test_remove_stale_top_level_adapter_stubs_deletes_generated_aliases(tmp_path):
    # Arrange
    root = tmp_path / "nautilus_trader"
    adapters_dir = root / "adapters"
    (adapters_dir / "polymarket").mkdir(parents=True)
    (adapters_dir / "polymarket" / "__init__.pyi").write_text("class Polymarket: ...\n")

    stale_dir = root / "polymarket"
    stale_dir.mkdir()
    stale_init = stale_dir / "__init__.pyi"
    stale_init.write_text("class Polymarket: ...\n")

    (adapters_dir / "bybit").mkdir()
    (adapters_dir / "bybit" / "__init__.pyi").write_text("class Bybit: ...\n")
    non_stale_dir = root / "bybit"
    non_stale_dir.mkdir()
    (non_stale_dir / "__init__.pyi").write_text("class Bybit: ...\n")
    (non_stale_dir / "extra.pyi").write_text("class Extra: ...\n")

    # Act
    generate_stubs.remove_stale_top_level_adapter_stubs(root)

    # Assert
    assert not stale_dir.exists()
    assert non_stale_dir.exists()


def test_sync_adapter_all_exports_copies_runtime_all_into_stub(tmp_path):
    # Arrange: runtime facade declares a curated __all__; stub has the raw generated one.
    root = tmp_path / "nautilus_trader"
    adapter_dir = root / "adapters" / "bybit"
    adapter_dir.mkdir(parents=True)
    (adapter_dir / "__init__.py").write_text(
        """
from nautilus_trader._libnautilus.bybit import *  # noqa: F403

__all__ = [
    "BYBIT",
    "BybitDataClientConfig",
]
""".lstrip(),
    )
    (adapter_dir / "__init__.pyi").write_text(
        """
__all__ = [
    "BYBIT",
    "BybitDataClientConfig",
    "BybitHttpClient",
    "BybitWebSocketClient",
]


class BybitDataClientConfig: ...
class BybitHttpClient: ...
class BybitWebSocketClient: ...
BYBIT: str
""".lstrip(),
    )

    # Act
    generate_stubs.sync_adapter_all_exports(root)

    # Assert
    stub = (adapter_dir / "__init__.pyi").read_text()
    exported = ast.literal_eval(
        next(
            node.value
            for node in ast.parse(stub).body
            if isinstance(node, ast.Assign)
            and any(
                isinstance(target, ast.Name) and target.id == "__all__" for target in node.targets
            )
        ),
    )
    assert exported == ["BYBIT", "BybitDataClientConfig"]
    # Definitions for non-exported members remain for explicit typed imports.
    assert "class BybitHttpClient: ..." in stub


def test_sync_adapter_all_exports_rejects_runtime_name_absent_from_stub(tmp_path):
    # Arrange: runtime exports a name the stub does not define or import.
    root = tmp_path / "nautilus_trader"
    adapter_dir = root / "adapters" / "binance"
    adapter_dir.mkdir(parents=True)
    (adapter_dir / "__init__.py").write_text(
        """
__all__ = ["BINANCE", "MissingSymbol"]
""".lstrip(),
    )
    (adapter_dir / "__init__.pyi").write_text(
        """
__all__ = ["BINANCE"]

BINANCE: str
""".lstrip(),
    )

    # Act / Assert
    with pytest.raises(ValueError, match=r"runtime __all__ names missing from stub"):
        generate_stubs.sync_adapter_all_exports(root)


def test_read_runtime_all_rejects_non_static_or_duplicated_list(tmp_path):
    # Arrange
    runtime_path = tmp_path / "__init__.py"
    runtime_path.write_text("__all__ = ['A', 'A', 'B']\n")

    # Act / Assert
    with pytest.raises(ValueError, match=r"__all__ must not contain duplicates"):
        generate_stubs._read_runtime_all(runtime_path)


def test_generated_stubs_do_not_expose_top_level_adapter_packages():
    # Arrange
    adapters_dir = STUB_ROOT / "adapters"

    # Act
    adapter_names = sorted(path.parent.name for path in adapters_dir.glob("*/__init__.pyi"))
    exposed = [
        adapter_name
        for adapter_name in adapter_names
        if (STUB_ROOT / adapter_name / "__init__.pyi").exists()
    ]

    # Assert
    assert not exposed


def test_add_names_to_all_inserts_sorted():
    # Arrange
    content = """
__all__ = [
    "Bravo",
    "Delta",
]
""".strip()

    # Act
    updated = generate_stubs._add_names_to_all(content, ["Alpha", "Charlie"])

    # Assert
    assert '"Alpha"' in updated
    assert '"Charlie"' in updated
    names = re.findall(r'"(\w+)"', updated)
    assert names == sorted(names)


def test_insert_constants_after_all():
    # Arrange
    content = """
__all__ = [
    "Foo",
]

class Foo:
    pass
""".strip()

    # Act
    updated = generate_stubs._insert_constants_after_all(content, "MY_CONST: int")

    # Assert
    assert "MY_CONST: int" in updated
    all_pos = updated.index("__all__")
    const_pos = updated.index("MY_CONST: int")
    class_pos = updated.index("class Foo:")
    assert all_pos < const_pos < class_pos


def test_binance_stub_exposes_python_migration_surface():
    stub = (STUB_ROOT / "adapters" / "binance" / "__init__.pyi").read_text()
    stub_module = ast.parse(stub)
    exported = next(
        ast.literal_eval(node.value)
        for node in stub_module.body
        if isinstance(node, ast.Assign)
        and any(isinstance(target, ast.Name) and target.id == "__all__" for target in node.targets)
    )

    expected = {
        "BINANCE",
        "BINANCE_CLIENT_ID",
        "BINANCE_VENUE",
        "decode_binance_futures_client_order_id",
        "decode_binance_spot_client_order_id",
        "load_binance_instruments",
        "load_binance_order_book_deltas",
    }
    functions = {node.name: node for node in stub_module.body if isinstance(node, ast.FunctionDef)}
    reexports = {
        (node.module, alias.name, alias.asname)
        for node in stub_module.body
        if isinstance(node, ast.ImportFrom)
        for alias in node.names
    }
    imports = {
        (alias.name, alias.asname)
        for node in stub_module.body
        if isinstance(node, ast.Import)
        for alias in node.names
    }

    assert expected <= set(exported)
    assert "BinanceOrderBookDeltaDataLoader" not in exported
    assert "BINANCE: str" in stub
    assert "BINANCE_CLIENT_ID: model.ClientId" in stub
    assert "BINANCE_VENUE: model.Venue" in stub
    assert (
        "nautilus_trader.adapters.binance.instruments",
        "load_binance_instruments",
        "load_binance_instruments",
    ) in reexports
    load = functions["load_binance_order_book_deltas"]
    assert load.decorator_list == []
    assert [argument.arg for argument in load.args.args] == ["file_path", "nrows"]
    assert ast.unparse(load.args.args[0].annotation) == "str | os.PathLike | pathlib.Path"
    assert ast.unparse(load.args.args[1].annotation) == "int | None"
    assert [ast.unparse(default) for default in load.args.defaults] == ["None"]
    assert ("pandas", "pd") in imports
    assert ast.unparse(load.returns) == "pd.DataFrame"
    assert len(load.body) == 1
    assert isinstance(load.body[0], ast.Expr)
    assert isinstance(load.body[0].value, ast.Constant)
    assert load.body[0].value.value is Ellipsis

    for name in (
        "decode_binance_futures_client_order_id",
        "decode_binance_spot_client_order_id",
    ):
        function = functions[name]
        assert [argument.arg for argument in function.args.args] == ["encoded"]
        assert ast.unparse(function.args.args[0].annotation) == "str"
        assert ast.unparse(function.returns) == "str"
        assert len(function.body) == 1
        assert isinstance(function.body[0], ast.Expr)
        assert isinstance(function.body[0].value, ast.Constant)
        assert function.body[0].value.value is Ellipsis


def test_fix_enum_defaults_in_signatures():
    # Arrange
    content = """
class AggregationSource(Enum):
    EXTERNAL = ...
    INTERNAL = ...

class BarType(Enum):
    Standard = ...
    Composite = ...

    def __init__(
        self,
        instrument_id: InstrumentId,
        spec: BarSpecification,
        aggregation_source: AggregationSource = AggregationSource.External,
    ) -> None: ...

class Strategy:
    def __init__(
        self,
        time_in_force: model.TimeInForce = model.TimeInForce.Gtc,
    ) -> None: ...
""".strip()
    renamed_enums = {"AggregationSource", "TimeInForce"}

    # Act
    updated = generate_stubs.fix_enum_defaults_in_signatures(content, renamed_enums)

    # Assert
    assert "AggregationSource.EXTERNAL" in updated
    assert "AggregationSource.External" not in updated
    assert "model.TimeInForce.GTC" in updated
    assert "model.TimeInForce.Gtc" not in updated
    # Non-renamed enum variants unchanged
    assert "Standard = ..." in updated


def test_elide_forward_class_defaults_in_signatures():
    content = """
class Client:
    def __init__(self, network: DydxNetwork = DydxNetwork.MAINNET) -> None: ...

    @staticmethod
    def from_env(
        environment: HyperliquidEnvironment = HyperliquidEnvironment.MAINNET,
        book_type: model.BookType = model.BookType.L1_MBP,
    ) -> Client: ...

class DydxNetwork(Enum):
    MAINNET = ...

class HyperliquidEnvironment(Enum):
    MAINNET = ...
""".strip()

    updated = generate_stubs.elide_forward_class_defaults_in_signatures(content)

    assert "network: DydxNetwork = ..." in updated
    assert "environment: HyperliquidEnvironment = ..." in updated
    assert "book_type: model.BookType = model.BookType.L1_MBP" in updated
    assert "DydxNetwork.MAINNET" not in updated
    assert "HyperliquidEnvironment.MAINNET" not in updated


def test_elide_forward_class_defaults_in_signatures_keeps_earlier_local_defaults():
    content = """
class BitmexEnvironment(Enum):
    MAINNET = ...

class Client:
    def __init__(
        self,
        environment: BitmexEnvironment = BitmexEnvironment.MAINNET,
    ) -> None: ...
""".strip()

    updated = generate_stubs.elide_forward_class_defaults_in_signatures(content)

    assert "environment: BitmexEnvironment = BitmexEnvironment.MAINNET" in updated


WORKSPACE_ROOT = Path(__file__).resolve().parents[3]
STUB_ROOT = WORKSPACE_ROOT / "python" / "nautilus_trader"

STUB_ENUM_CLASS_RE = re.compile(r"^class\s+(\w+)\s*\(\s*(?:enum\.)?Enum\s*\)\s*:")
STUB_VARIANT_RE = re.compile(r"^\s+([A-Za-z_]\w*)\s*=\s*\.\.\.")
STUB_CONFIG_CLASS_RE = re.compile(r"^class\s+([A-Za-z_]\w*Config)\b", re.MULTILINE)
RUST_CONFIG_STRUCT_RE = re.compile(
    r"^\s*pub\s+struct\s+([A-Za-z_]\w*Config)\b",
    re.MULTILINE,
)
RUST_CONFIG_IMPL_RE = re.compile(
    r"^\s*impl\s+([A-Za-z_]\w*Config)\s*\{",
    re.MULTILINE,
)
RUST_CONFIG_GETTER_MACRO_RE = re.compile(
    r"impl_pyo3_config_getters!\(\s*([A-Za-z_]\w*Config)\s*\{",
)
RUST_CONFIG_MACRO_INVOCATION_RE = re.compile(
    r"^\s*([A-Za-z_]\w*)!\(\s*([A-Za-z_]\w*Config)\s*\);\s*$",
    re.MULTILINE,
)
RUST_MACRO_RE = re.compile(r"^\s*macro_rules!\s+([A-Za-z_]\w*)\s*\{", re.MULTILINE)
RUST_STRUCT_FIELD_RE = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?([A-Za-z_]\w*)\s*:",
    re.MULTILINE,
)
RUST_FUNCTION_RE = re.compile(
    r"\b(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?fn\s+([A-Za-z_]\w*)\s*(?:<[^>{}]+>)?\s*\(",
)
PYO3_SIGNATURE_RE = re.compile(r"#\[pyo3\(signature\s*=\s*\((.*?)\)\)\]", re.DOTALL)

CONFIG_READBACK_REPLACEMENTS = {
    (
        "nautilus_trader.backtest",
        "BacktestDataConfig",
        "catalog_fs_storage_options",
    ): "catalog_fs_storage_option_keys",
    (
        "nautilus_trader.backtest",
        "BacktestDataConfig",
        "catalog_fs_rust_storage_options",
    ): "catalog_fs_rust_storage_option_keys",
    ("nautilus_trader.network", "SocketConfig", "handler"): "has_handler",
    ("nautilus_trader.network", "WebSocketConfig", "headers"): "header_names",
    ("nautilus_trader.network", "WebSocketConfig", "proxy_url"): "has_proxy_url",
}

WRITABLE_CONFIG_PROPERTIES = {
    ("nautilus_trader.common", "DataActorConfig"): {
        "actor_id",
        "log_commands",
        "log_events",
    },
    ("nautilus_trader.adapters.interactive_brokers", "InteractiveBrokersDataClientConfig"): {
        "instrument_provider",
    },
    ("nautilus_trader.adapters.interactive_brokers", "InteractiveBrokersExecClientConfig"): {
        "instrument_provider",
    },
    (
        "nautilus_trader.adapters.interactive_brokers",
        "InteractiveBrokersInstrumentProviderConfig",
    ): {"cache_path"},
}

# Generated stubs intentionally omit every public property on these wire-schema,
# acknowledgement, and error DTOs. They are confirmed non-contract runtime members
# under the plan property-audit "adapter wire-schema DTOs" classification, so the
# bidirectional guard does not require stub coverage for them. Reduce this set only
# after the owning crate exposes a member as part of the supported public contract.
NON_CONTRACT_DTO_CLASSES = frozenset(
    {
        ("nautilus_trader.adapters.bybit", "BybitAccountDetails"),
        ("nautilus_trader.adapters.bybit", "BybitFeeRate"),
        ("nautilus_trader.adapters.bybit", "BybitNativeTpSlParams"),
        ("nautilus_trader.adapters.bybit", "BybitOrder"),
        ("nautilus_trader.adapters.bybit", "BybitOrderCursorList"),
        ("nautilus_trader.adapters.bybit", "BybitServerTime"),
        ("nautilus_trader.adapters.bybit", "BybitTickerData"),
        ("nautilus_trader.adapters.bybit", "BybitTickersParams"),
        ("nautilus_trader.adapters.bybit", "BybitWsAmendOrderParams"),
        ("nautilus_trader.adapters.bybit", "BybitWsCancelOrderParams"),
        ("nautilus_trader.adapters.bybit", "BybitWsPlaceOrderParams"),
        ("nautilus_trader.adapters.databento", "DatabentoSubscriptionAck"),
        ("nautilus_trader.adapters.okx", "OKXWebSocketError"),
    },
)

# Runtime methods absent from generated stubs pending a source or generator fix.
# The bidirectional guard surfaces each as a real contract gap; they are deferred
# until Unit 7 checkbox 3 resolves macro, complex-signature, and classmethod stub
# generation. Do not add entries here without a concrete deferral reason.
DEFERRED_RUNTIME_METHODS = frozenset(
    {
        ("nautilus_trader.adapters.kraken", "KrakenFuturesHttpClient", "edit_orders_batch"),
        ("nautilus_trader.adapters.kraken", "KrakenFuturesHttpClient", "submit_orders_batch"),
        ("nautilus_trader.adapters.kraken", "KrakenSpotHttpClient", "submit_orders_batch"),
        ("nautilus_trader.model", "CustomData", "from_json_bytes"),
        ("nautilus_trader.model", "Price", "as_double"),
    },
)

ADAPTER_CONFIG_SECRET_FIELDS = {
    "api_key",
    "api_secret",
    "api_passphrase",
    "app_key",
    "password",
    "passphrase",
    "private_key",
    "session_key",
}
ADAPTER_CONFIG_READBACK_REPLACEMENTS = {
    "proxy_url": "has_proxy_url",
    "submitter_proxy_urls": "has_submitter_proxy_urls",
    "canceller_proxy_urls": "has_canceller_proxy_urls",
}
ADAPTER_CONFIG_FIELD_READBACK_REPLACEMENTS = {
    (
        "nautilus_trader.adapters.blockchain",
        "BlockchainDataClientConfig",
        "postgres_cache_database_config",
    ): "has_postgres_cache_database_config",
    (
        "nautilus_trader.adapters.interactive_brokers",
        "DockerizedIBGatewayConfig",
        "password",
    ): "has_password",
}
ADAPTER_CONFIG_CONSTRUCTOR_ONLY_FIELDS = {
    (
        "nautilus_trader.adapters.interactive_brokers",
        "InteractiveBrokersDataClientConfig",
        "dockerized_gateway",
    ),
    (
        "nautilus_trader.adapters.interactive_brokers",
        "InteractiveBrokersExecClientConfig",
        "dockerized_gateway",
    ),
}


def _parse_stub_enum_variants(stub_root: Path) -> dict[str, list[str]]:
    """
    Parse all .pyi files and return enum_name -> list of variant names.
    """
    result: dict[str, list[str]] = {}

    for pyi in sorted(stub_root.rglob("*.pyi")):
        current_enum: str | None = None

        for line in pyi.read_text().splitlines():
            class_match = STUB_ENUM_CLASS_RE.match(line)
            if class_match:
                current_enum = class_match.group(1)
                result.setdefault(current_enum, [])
                continue

            if current_enum is not None:
                variant_match = STUB_VARIANT_RE.match(line)
                if variant_match:
                    result[current_enum].append(variant_match.group(1))
                elif line.strip() and not line[0].isspace():
                    current_enum = None

    return result


SCREAMING_SNAKE_RE = re.compile(r"^[A-Z][A-Z0-9]*(_[A-Z0-9]+)*_?$")


def test_live_stub_exposes_native_live_node_config_signature():
    live_stub = (STUB_ROOT / "live" / "__init__.pyi").read_text()

    assert "@typing.final\nclass LiveNodeConfig:" in live_stub
    assert re.search(
        r"portfolio:\s+(?:portfolio\.)?PortfolioConfig \| None = None",
        live_stub,
    )
    assert '"PortfolioConfig"' in live_stub


def test_live_stub_exposes_builder_engine_config_methods():
    live_stub = (STUB_ROOT / "live" / "__init__.pyi").read_text()

    assert (
        "def with_cache_config(self, config: common.CacheConfig) -> LiveNodeBuilder: ..."
        in live_stub
    )
    assert (
        "def with_portfolio_config(self, config: portfolio.PortfolioConfig) -> LiveNodeBuilder: ..."
        in live_stub
    )
    assert (
        "def with_data_engine_config(self, config: LiveDataEngineConfig) -> LiveNodeBuilder: ..."
        in live_stub
    )
    assert (
        "def with_risk_engine_config(self, config: LiveRiskEngineConfig) -> LiveNodeBuilder: ..."
        in live_stub
    )
    assert (
        "def with_exec_engine_config(self, config: LiveExecEngineConfig) -> LiveNodeBuilder: ..."
        in live_stub
    )


@pytest.mark.parametrize(
    ("module_name", "class_name"),
    [
        ("nautilus_trader.adapters.dydx", "DydxClientOrderIdEncoder"),
        ("nautilus_trader.persistence", "DataBackendSession"),
        ("nautilus_trader.persistence", "ParquetDataCatalog"),
        ("nautilus_trader.persistence", "StreamingFeatherWriter"),
    ],
)
def test_stub_constructor_matches_runtime(module_name, class_name):
    runtime_class = getattr(importlib.import_module(module_name), class_name)
    stub_path = STUB_ROOT.joinpath(*module_name.split(".")[1:], "__init__.pyi")
    stub_module = ast.parse(stub_path.read_text())
    stub_class = next(
        node
        for node in stub_module.body
        if isinstance(node, ast.ClassDef) and node.name == class_name
    )
    methods = {node.name: node for node in stub_class.body if isinstance(node, ast.FunctionDef)}

    assert "__init__" in methods
    assert "new" not in methods
    assert "new_session" not in methods

    constructor = methods["__init__"]
    stub_parameters = [argument.arg for argument in constructor.args.args[1:]]
    runtime_signature = inspect.signature(runtime_class)
    runtime_parameters = list(runtime_signature.parameters)
    stub_default_parameters = (
        stub_parameters[-len(constructor.args.defaults) :] if constructor.args.defaults else []
    )
    stub_defaults = {
        name: ast.literal_eval(default)
        for name, default in zip(
            stub_default_parameters,
            constructor.args.defaults,
            strict=True,
        )
    }
    runtime_default_parameters = [
        name
        for name, parameter in runtime_signature.parameters.items()
        if parameter.default is not inspect.Parameter.empty
    ]
    runtime_defaults = {
        name: runtime_signature.parameters[name].default for name in runtime_default_parameters
    }

    assert stub_parameters == runtime_parameters
    assert stub_default_parameters == runtime_default_parameters
    assert stub_defaults == runtime_defaults


def test_stub_members_match_runtime_names():
    forward_missing = []
    reverse_missing = []
    kind_mismatches = []
    raw_runtime_names = []

    for stub_path in sorted(STUB_ROOT.rglob("__init__.pyi")):
        relative_package = stub_path.relative_to(STUB_ROOT).parent
        if any(part.startswith("_") for part in relative_package.parts):
            continue

        module_name = _module_name_from_stub_path(relative_package)
        module = importlib.import_module(module_name)
        stub_module = ast.parse(stub_path.read_text())
        runtime_names = set(dir(module))
        stub_names = {
            node.name
            for node in stub_module.body
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
        }
        forward_missing.extend(
            f"{module_name}.{name}"
            for name in sorted(_stub_names_missing_at_runtime(stub_names, runtime_names))
        )
        raw_runtime_names.extend(
            f"{module_name}.{name}" for name in sorted(runtime_names) if name.startswith("py_")
        )

        for stub_class in (node for node in stub_module.body if isinstance(node, ast.ClassDef)):
            runtime_class = getattr(module, stub_class.name, None)
            if not isinstance(runtime_class, type):
                forward_missing.append(f"{module_name}.{stub_class.name}")
                continue

            class_key = (module_name, stub_class.name)
            runtime_names = set(dir(runtime_class))
            stub_kinds = _stub_member_kinds(stub_class.body, class_scope=True)
            stub_names = set(stub_kinds)
            forward_missing.extend(
                f"{module_name}.{stub_class.name}.{name}"
                for name in sorted(_stub_names_missing_at_runtime(stub_names, runtime_names))
            )
            raw_runtime_names.extend(
                f"{module_name}.{stub_class.name}.{name}"
                for name in sorted(runtime_names)
                if name.startswith("py_")
            )
            _collect_reverse_member_drift(
                class_key,
                runtime_class,
                stub_kinds,
                reverse_missing,
                kind_mismatches,
            )

    assert not forward_missing, (
        "Stub members missing at runtime; register intended public APIs or remove stale stub "
        "metadata:\n" + "\n".join(forward_missing)
    )
    assert not reverse_missing, (
        "Runtime members missing from stubs; add the binding at the PyO3 source and regenerate, "
        "or register confirmed non-contract members in NON_CONTRACT_DTO_CLASSES:\n"
        + "\n".join(reverse_missing)
    )
    assert not kind_mismatches, (
        "Stub and runtime members differ in property-versus-method kind:\n"
        + "\n".join(
            kind_mismatches,
        )
    )
    assert not raw_runtime_names, "Raw Rust names exposed at runtime:\n" + "\n".join(
        raw_runtime_names,
    )


def _stub_names_missing_at_runtime(stub_names, runtime_names):
    return {
        name
        for name in stub_names
        if name not in runtime_names
        and not (name.endswith("_") and keyword.iskeyword(name[:-1]) and name[:-1] in runtime_names)
    }


def _stub_function_kind(fn):
    for decorator in fn.decorator_list:
        if isinstance(decorator, ast.Name) and decorator.id == "property":
            return "property"
        if isinstance(decorator, ast.Attribute):
            if decorator.attr == "property":
                return "property"
            if decorator.attr in {"setter", "deleter"}:
                return "accessor"
    return "method"


def _stub_member_kinds(stub_body, class_scope):
    kinds: dict[str, str] = {}

    for node in stub_body:
        if not isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            continue
        if class_scope:
            if node.name.startswith("__") and node.name not in {"__init__", "__new__"}:
                continue
        elif node.name.startswith("__"):
            continue
        kind = _stub_function_kind(node)
        if kind == "property":
            kinds[node.name] = "property"
        elif kind == "method" and kinds.get(node.name) != "property":
            kinds[node.name] = "method"
    return kinds


def _runtime_member_kind(runtime_class, name):
    obj = inspect.getattr_static(runtime_class, name, None)
    if obj is None:
        return None
    type_name = type(obj).__name__
    if isinstance(obj, property) or type_name == "getset_descriptor":
        return "property"
    if type_name in {
        "method_descriptor",
        "classmethod_descriptor",
        "staticmethod_descriptor",
        "function",
        "builtin_function_or_method",
        "method",
    }:
        return "method"
    if inspect.isfunction(obj) or inspect.ismethod(obj) or inspect.ismethoddescriptor(obj):
        return "method"
    return None


def _keyword_alias(name):
    return f"{name}_" if keyword.iskeyword(name) else None


def _collect_reverse_member_drift(
    class_key,
    runtime_class,
    stub_kinds,
    reverse_missing,
    kind_mismatches,
):
    module_name, class_name = class_key
    non_contract_dto = class_key in NON_CONTRACT_DTO_CLASSES

    for name in dir(runtime_class):
        if name.startswith("_"):
            continue
        runtime_kind = _runtime_member_kind(runtime_class, name)
        if runtime_kind is None:
            continue
        alias = _keyword_alias(name)
        stub_name = alias if alias is not None and alias in stub_kinds else name
        if stub_name in stub_kinds:
            stub_kind = stub_kinds[stub_name]
            if stub_kind != runtime_kind:
                kind_mismatches.append(
                    f"{module_name}.{class_name}.{name}: runtime {runtime_kind} vs stub {stub_kind}",
                )
            continue
        if runtime_kind == "property" and non_contract_dto:
            continue
        if runtime_kind == "method" and (module_name, class_name, name) in DEFERRED_RUNTIME_METHODS:
            continue
        reverse_missing.append(f"{module_name}.{class_name}.{name} ({runtime_kind})")


def test_collect_reverse_member_drift_reports_gaps_and_respects_allowlists(monkeypatch):
    class Runtime:
        @property
        def matched_prop(self) -> int:
            return 1

        def matched_method(self) -> None:
            pass

        @property
        def drifted_to_method(self) -> int:
            return 1

        @property
        def allowlisted_prop(self) -> int:
            return 1

        def deferred_method(self) -> None:
            pass

        def unreported_gap(self) -> None:
            pass

    stub = ast.parse(
        "class Runtime:\n"
        "    @property\n    def matched_prop(self) -> int: ...\n"
        "    def matched_method(self) -> None: ...\n"
        "    def drifted_to_method(self) -> None: ...\n",
    ).body[0]
    stub_kinds = _stub_member_kinds(stub.body, class_scope=True)

    key = ("ns", "Runtime")
    module = sys.modules[__name__]
    monkeypatch.setattr(module, "NON_CONTRACT_DTO_CLASSES", frozenset({key}))
    monkeypatch.setattr(module, "DEFERRED_RUNTIME_METHODS", frozenset({(*key, "deferred_method")}))

    reverse_missing = []
    kind_mismatches = []
    _collect_reverse_member_drift(key, Runtime, stub_kinds, reverse_missing, kind_mismatches)

    assert reverse_missing == ["ns.Runtime.unreported_gap (method)"]
    assert kind_mismatches == [
        "ns.Runtime.drifted_to_method: runtime property vs stub method",
    ]


def test_collect_reverse_member_drift_resolves_keyword_alias():
    class TransactionLike:
        pass

    setattr(TransactionLike, "from", property(lambda self: "x"))

    stub = ast.parse(
        "class TransactionLike:\n    @property\n    def from_(self) -> str: ...\n",
    ).body[0]
    stub_kinds = _stub_member_kinds(stub.body, class_scope=True)

    reverse_missing = []
    kind_mismatches = []
    _collect_reverse_member_drift(
        ("ns", "TransactionLike"),
        TransactionLike,
        stub_kinds,
        reverse_missing,
        kind_mismatches,
    )

    assert reverse_missing == []
    assert kind_mismatches == []


def test_pylist_construction_propagates_pyresult_collections():
    violations = []
    list_pattern = re.compile(r"PyList::new\(py,\s*(\w+)(\?)?\)")

    for rust_path in sorted((WORKSPACE_ROOT / "crates").rglob("*.rs")):
        lines = rust_path.read_text(encoding="utf-8").splitlines()
        for index, line in enumerate(lines):
            match = list_pattern.search(line)
            if match is None or match.group(2) is not None:
                continue

            binding = re.escape(match.group(1))
            preceding_lines = "\n".join(lines[max(0, index - 12) : index])
            if re.search(rf"let\s+{binding}\s*:\s*PyResult<Vec<_>>", preceding_lines):
                relative_path = rust_path.relative_to(WORKSPACE_ROOT)
                violations.append(f"{relative_path}:{index + 1}")

    assert not violations, "PyResult collections passed to PyList without `?`:\n" + "\n".join(
        violations,
    )


def test_stub_signatures_match_runtime():
    parameter_mismatches = []
    default_mismatches = []

    for stub_path in sorted(STUB_ROOT.rglob("__init__.pyi")):
        relative_package = stub_path.relative_to(STUB_ROOT).parent
        if any(part.startswith("_") for part in relative_package.parts):
            continue

        module_name = _module_name_from_stub_path(relative_package)
        module = importlib.import_module(module_name)
        stub_module = ast.parse(stub_path.read_text())
        parameter_errors, default_errors = _module_signature_mismatches(
            module_name,
            stub_module,
            module,
        )
        parameter_mismatches.extend(parameter_errors)
        default_mismatches.extend(default_errors)

        for stub_class in (node for node in stub_module.body if isinstance(node, ast.ClassDef)):
            runtime_class = getattr(module, stub_class.name, None)
            if not isinstance(runtime_class, type):
                continue

            parameter_errors, default_errors = _method_signature_mismatches(
                module_name,
                stub_class,
                runtime_class,
            )
            parameter_mismatches.extend(parameter_errors)
            default_mismatches.extend(default_errors)

    assert not parameter_mismatches, "Stub parameter mismatches:\n" + "\n".join(
        parameter_mismatches,
    )
    assert not default_mismatches, "Stub default mismatches:\n" + "\n".join(default_mismatches)


def _module_signature_mismatches(module_name, stub_module, module):
    parameter_mismatches = []
    default_mismatches = []

    for function_name, functions in _stub_methods_by_name(stub_module).items():
        if len(functions) != 1:
            continue

        function = functions[0]
        runtime_function = getattr(module, function_name)
        try:
            runtime_signature = inspect.signature(runtime_function)
        except (TypeError, ValueError):
            continue

        stub_parameters, runtime_parameters = _normalized_parameter_names(
            _stub_parameter_names(function),
            list(runtime_signature.parameters),
        )
        label = f"{module_name}.{function_name}"

        if stub_parameters != runtime_parameters:
            parameter_mismatches.append(
                f"{label}: stub={stub_parameters}, runtime={runtime_parameters}",
            )
            continue

        for name, stub_default in _concrete_stub_defaults(function).items():
            runtime_default = runtime_signature.parameters[name].default
            if runtime_default is Ellipsis:
                continue
            if runtime_default is inspect.Parameter.empty or stub_default != runtime_default:
                default_mismatches.append(
                    f"{label}.{name}: stub={stub_default!r}, runtime={runtime_default!r}",
                )

    return parameter_mismatches, default_mismatches


def _method_signature_mismatches(module_name, stub_class, runtime_class):
    parameter_mismatches = []
    default_mismatches = []

    for method_name, methods in _stub_methods_by_name(stub_class).items():
        if len(methods) != 1 or (method_name.startswith("__") and method_name != "__init__"):
            continue

        method = methods[0]
        decorators = {ast.unparse(decorator) for decorator in method.decorator_list}
        if "property" in decorators or any(
            decorator.endswith(".setter") for decorator in decorators
        ):
            continue

        runtime_method = (
            runtime_class
            if method_name == "__init__"
            else getattr(runtime_class, method_name, None)
        )
        try:
            runtime_signature = inspect.signature(runtime_method)
        except (TypeError, ValueError):
            continue

        stub_parameters, runtime_parameters = _normalized_parameter_names(
            _stub_parameter_names(method),
            list(runtime_signature.parameters),
        )
        label = f"{module_name}.{stub_class.name}.{method_name}"

        if stub_parameters != runtime_parameters:
            parameter_mismatches.append(
                f"{label}: stub={stub_parameters}, runtime={runtime_parameters}",
            )
            continue

        for name, stub_default in _concrete_stub_defaults(method).items():
            runtime_default = runtime_signature.parameters[name].default
            if runtime_default is Ellipsis:
                continue
            if runtime_default is inspect.Parameter.empty or stub_default != runtime_default:
                default_mismatches.append(
                    f"{label}.{name}: stub={stub_default!r}, runtime={runtime_default!r}",
                )

    return parameter_mismatches, default_mismatches


def _stub_methods_by_name(stub_class: ast.ClassDef | ast.Module):
    methods = {}

    for node in stub_class.body:
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            methods.setdefault(node.name, []).append(node)
    return methods


def _stub_parameter_names(method: ast.FunctionDef | ast.AsyncFunctionDef):
    names = [argument.arg for argument in method.args.posonlyargs + method.args.args]
    if method.args.vararg:
        names.append(method.args.vararg.arg)
    names.extend(argument.arg for argument in method.args.kwonlyargs)
    if method.args.kwarg:
        names.append(method.args.kwarg.arg)
    return names


def _normalized_parameter_names(stub_parameters, runtime_parameters):
    if stub_parameters and stub_parameters[0] in {"self", "cls"}:
        if not runtime_parameters or runtime_parameters[0] not in {"self", "cls"}:
            stub_parameters = stub_parameters[1:]
    elif runtime_parameters and runtime_parameters[0] in {"self", "cls"}:
        runtime_parameters = runtime_parameters[1:]
    return stub_parameters, runtime_parameters


def _concrete_stub_defaults(method: ast.FunctionDef | ast.AsyncFunctionDef):
    positional_arguments = method.args.posonlyargs + method.args.args
    positional_defaults = [None] * (len(positional_arguments) - len(method.args.defaults)) + list(
        method.args.defaults,
    )
    default_nodes = positional_defaults + list(method.args.kw_defaults)
    default_names = [argument.arg for argument in positional_arguments + method.args.kwonlyargs]
    defaults = {}

    for name, default_node in zip(default_names, default_nodes, strict=True):
        if default_node is None or (
            isinstance(default_node, ast.Constant) and default_node.value is Ellipsis
        ):
            continue
        try:
            defaults[name] = ast.literal_eval(default_node)
        except (TypeError, ValueError):
            continue

    return defaults


def test_generated_config_stubs_include_signature_defaults():
    rust_fixups = generate_stubs.collect_rust_class_fixups(WORKSPACE_ROOT)
    renamed_enums = generate_stubs.collect_renamed_enums(WORKSPACE_ROOT)
    mismatches = []

    for stub_file in sorted(STUB_ROOT.rglob("*.pyi")):
        content = stub_file.read_text()
        config_fixups = _config_constructor_fixups_for_stub(content, rust_fixups)
        if not config_fixups:
            continue

        updated = generate_stubs.apply_signature_defaults(content, config_fixups)
        updated = generate_stubs.fix_enum_defaults_in_signatures(updated, renamed_enums)
        updated = generate_stubs.elide_forward_class_defaults_in_signatures(updated)

        if updated != content:
            mismatches.append(stub_file.relative_to(WORKSPACE_ROOT).as_posix())

    assert mismatches == [], "Run `make py-stubs-v2`; stale config defaults in " + ", ".join(
        mismatches,
    )


def _iter_supported_stub_configs(adapter):
    for stub_file in sorted(STUB_ROOT.rglob("__init__.pyi")):
        relative_package = stub_file.relative_to(STUB_ROOT).parent
        module_name = _module_name_from_stub_path(relative_package)
        is_adapter = module_name.startswith("nautilus_trader.adapters.")
        if adapter is not None and is_adapter != adapter:
            continue

        for stub_class in (
            node
            for node in ast.parse(stub_file.read_text()).body
            if isinstance(node, ast.ClassDef) and node.name.endswith("Config")
        ):
            if any(
                isinstance(node, ast.FunctionDef) and node.name in {"__init__", "__new__"}
                for node in stub_class.body
            ):
                yield module_name, stub_class


def test_non_adapter_config_constructors_have_runtime_readback():
    mismatches = []

    for module_name, stub_class in _iter_supported_stub_configs(adapter=False):
        module = importlib.import_module(module_name)
        mismatches.extend(
            _config_constructor_readback_mismatches(module_name, module, stub_class),
        )

    assert mismatches == [], "Non-adapter config readback drift:\n" + "\n".join(mismatches)


def test_adapter_config_constructors_have_runtime_readback():
    mismatches = []

    for module_name, stub_class in _iter_supported_stub_configs(adapter=True):
        module = importlib.import_module(module_name)
        mismatches.extend(
            _config_constructor_readback_mismatches(
                module_name,
                module,
                stub_class,
            ),
        )

    assert mismatches == [], "Adapter config readback drift:\n" + "\n".join(mismatches)


def test_adapter_config_readback_returns_constructor_values(tmp_path):
    from nautilus_trader.adapters.architect_ax import AxDataClientConfig
    from nautilus_trader.adapters.betfair import BetfairDataConfig
    from nautilus_trader.adapters.bitmex import BitmexExecClientConfig
    from nautilus_trader.adapters.bitmex import BitmexExecFactoryConfig
    from nautilus_trader.adapters.bybit import BybitDataClientConfig
    from nautilus_trader.adapters.databento import DatabentoLiveClientConfig
    from nautilus_trader.model import AccountId
    from nautilus_trader.model import TraderId

    ax_config = AxDataClientConfig(
        base_url_http="https://ax.example.test",
        proxy_url="http://user:password@proxy.example.test",
        http_timeout_secs=17,
    )
    betfair_config = BetfairDataConfig(
        username="readback-user",
        password="readback-password",
        app_key="readback-app-key",
        proxy_url="http://user:password@proxy.example.test",
        event_type_ids=[7, 9],
        stream_heartbeat_ms=4321,
    )
    bitmex_config = BitmexExecClientConfig(
        submitter_proxy_urls=["http://submitter.example.test"],
        canceller_proxy_urls=["http://canceller.example.test"],
        deadmans_switch_timeout_secs=45,
    )
    bybit_config = BybitDataClientConfig(instrument_status_poll_secs=23)
    databento_config = DatabentoLiveClientConfig(
        api_key="readback-api-key",
        publishers_filepath=tmp_path / "publishers.json",
        use_exchange_as_venue=True,
        bars_timestamp_on_close=False,
        venue_dataset_map={"XNAS": "XNAS.ITCH"},
    )
    factory_config = BitmexExecFactoryConfig(
        trader_id=TraderId("TRADER-001"),
        account_id=AccountId("BITMEX-001"),
        config=bitmex_config,
    )

    assert ax_config.base_url_http == "https://ax.example.test"
    assert ax_config.http_timeout_secs == 17
    assert ax_config.has_proxy_url is True
    assert betfair_config.username == "readback-user"
    assert betfair_config.event_type_ids == ["7", "9"]
    assert betfair_config.stream_heartbeat_ms == 4321
    assert betfair_config.has_proxy_url is True
    assert bitmex_config.deadmans_switch_timeout_secs == 45
    assert bitmex_config.has_submitter_proxy_urls is True
    assert bitmex_config.has_canceller_proxy_urls is True
    assert bybit_config.instrument_status_poll_secs == 23
    assert databento_config.publishers_filepath == tmp_path / "publishers.json"
    assert databento_config.use_exchange_as_venue is True
    assert databento_config.bars_timestamp_on_close is False
    assert databento_config.venue_dataset_map == {"XNAS": "XNAS.ITCH"}
    assert factory_config.trader_id == TraderId("TRADER-001")
    assert factory_config.account_id == AccountId("BITMEX-001")
    assert factory_config.config.deadmans_switch_timeout_secs == 45


def test_adapter_config_runtime_setter_policy(tmp_path):
    from nautilus_trader.adapters.architect_ax import AxDataClientConfig
    from nautilus_trader.adapters.interactive_brokers import DockerizedIBGatewayConfig
    from nautilus_trader.adapters.interactive_brokers import InteractiveBrokersDataClientConfig
    from nautilus_trader.adapters.interactive_brokers import InteractiveBrokersExecClientConfig
    from nautilus_trader.adapters.interactive_brokers import (
        InteractiveBrokersInstrumentProviderConfig,
    )

    readonly_config = AxDataClientConfig(base_url_http="https://ax.example.test")
    gateway_config = DockerizedIBGatewayConfig()
    provider_config = InteractiveBrokersInstrumentProviderConfig()
    data_config = InteractiveBrokersDataClientConfig()
    exec_config = InteractiveBrokersExecClientConfig()

    with pytest.raises(AttributeError):
        readonly_config.base_url_http = "https://changed.example.test"
    for config_class in (InteractiveBrokersDataClientConfig, InteractiveBrokersExecClientConfig):
        with pytest.raises(ValueError, match="is not wired into the Rust/PyO3 IB"):
            config_class(dockerized_gateway=gateway_config)

    provider_config.cache_path = str(tmp_path)
    data_config.instrument_provider = provider_config
    exec_config.instrument_provider = provider_config

    assert data_config.instrument_provider.cache_path == str(tmp_path)
    assert exec_config.instrument_provider.cache_path == str(tmp_path)
    assert provider_config.cache_path == str(tmp_path)


def test_adapter_config_secret_values_are_not_exposed(tmp_path):
    from nautilus_trader.model import AccountId
    from nautilus_trader.model import TraderId

    required_values = {
        "account_id": AccountId("VENUE-001"),
        "publishers_filepath": tmp_path / "publishers.json",
        "trader_id": TraderId("TRADER-001"),
    }
    failures = []

    for stub_file in sorted((STUB_ROOT / "adapters").glob("*/__init__.pyi")):
        relative_package = stub_file.relative_to(STUB_ROOT).parent
        module_name = _module_name_from_stub_path(relative_package)
        module = importlib.import_module(module_name)

        for stub_class in (
            node
            for node in ast.parse(stub_file.read_text()).body
            if isinstance(node, ast.ClassDef) and node.name.endswith("Config")
        ):
            runtime_class = getattr(module, stub_class.name)
            signature = inspect.signature(runtime_class)
            secret_parameters = [
                name for name in signature.parameters if name in ADAPTER_CONFIG_SECRET_FIELDS
            ]

            if not secret_parameters:
                continue

            kwargs = {name: f"raw-secret-{name}" for name in secret_parameters}
            for parameter in signature.parameters.values():
                if parameter.default is not inspect.Parameter.empty or parameter.name in kwargs:
                    continue
                if parameter.name not in required_values:
                    failures.append(
                        f"{module_name}.{stub_class.name}: no value for required {parameter.name}",
                    )
                    break
                kwargs[parameter.name] = required_values[parameter.name]
            else:
                config = runtime_class(**kwargs)
                representations = (repr(config), str(config))

                for parameter_name in secret_parameters:
                    secret_value = kwargs[parameter_name]
                    if inspect.getattr_static(runtime_class, parameter_name, None) is not None:
                        failures.append(
                            f"{module_name}.{stub_class.name}.{parameter_name}: raw property exists",
                        )
                    if any(secret_value in representation for representation in representations):
                        failures.append(
                            f"{module_name}.{stub_class.name}.{parameter_name}: value in representation",
                        )

    assert failures == [], "Adapter config secret policy drift:\n" + "\n".join(failures)


def test_adapter_config_sensitive_readback_values_are_not_represented():
    from nautilus_trader.adapters.bitmex import BitmexExecClientConfig
    from nautilus_trader.adapters.blockchain import BlockchainDataClientConfig
    from nautilus_trader.adapters.derive import DeriveDataClientConfig
    from nautilus_trader.adapters.dydx import DydxDataClientConfig
    from nautilus_trader.infrastructure import PostgresConnectOptions
    from nautilus_trader.model import Chain
    from nautilus_trader.model import DexType

    sentinel = "raw-sensitive-value"
    configs = [
        BitmexExecClientConfig(
            submitter_proxy_urls=[f"http://{sentinel}@submitter.example.test"],
            canceller_proxy_urls=[f"http://{sentinel}@canceller.example.test"],
        ),
        BlockchainDataClientConfig(
            chain=Chain.ARBITRUM(),
            dex_ids=[DexType.UNISWAP_V3],
            http_rpc_url="https://arb1.arbitrum.io/rpc",
            postgres_cache_database_config=PostgresConnectOptions(
                host="localhost",
                port=5432,
                user="user",
                password=sentinel,
                database="database",
            ),
            proxy_url=f"http://{sentinel}@proxy.example.test",
        ),
        DeriveDataClientConfig(proxy_url=f"http://{sentinel}@proxy.example.test"),
        DydxDataClientConfig(proxy_url=f"http://{sentinel}@proxy.example.test"),
    ]

    assert all(sentinel not in repr(config) for config in configs)
    assert all(sentinel not in str(config) for config in configs)


def _config_constructor_readback_mismatches(
    module_name,
    module,
    stub_class,
):
    constructor = next(
        (
            node
            for node in stub_class.body
            if isinstance(node, ast.FunctionDef) and node.name in {"__init__", "__new__"}
        ),
        None,
    )

    if constructor is None:
        return []

    mismatches = []
    runtime_class = getattr(module, stub_class.name)
    properties = {
        node.name
        for node in stub_class.body
        if isinstance(node, ast.FunctionDef)
        and any(
            isinstance(decorator, ast.Name) and decorator.id == "property"
            for decorator in node.decorator_list
        )
    }
    setters = {
        node.name
        for node in stub_class.body
        if isinstance(node, ast.FunctionDef)
        and any(
            isinstance(decorator, ast.Attribute) and decorator.attr == "setter"
            for decorator in node.decorator_list
        )
    }
    expected_setters = WRITABLE_CONFIG_PROPERTIES.get((module_name, stub_class.name), set())
    if setters != expected_setters:
        mismatches.append(
            f"{module_name}.{stub_class.name}: setters {sorted(setters)}, "
            f"expected {sorted(expected_setters)}",
        )

    positional_parameters = [*constructor.args.posonlyargs, *constructor.args.args]
    parameters = [*positional_parameters[1:], *constructor.args.kwonlyargs]
    for parameter in parameters:
        if parameter.arg.startswith("_"):
            continue

        field_key = (module_name, stub_class.name, parameter.arg)

        if module_name.startswith("nautilus_trader.adapters."):
            readback_name, policy_mismatches = _adapter_config_field_policy(
                runtime_class,
                properties,
                field_key,
            )
            mismatches.extend(policy_mismatches)

            if readback_name is None:
                continue
        else:
            readback_name = _config_readback_name(field_key)

        mismatches.extend(
            _config_readback_descriptor_mismatches(
                runtime_class,
                properties,
                field_key,
                readback_name,
            ),
        )

    return mismatches


def _config_readback_name(field_key):
    module_name, _, field_name = field_key
    if not module_name.startswith("nautilus_trader.adapters."):
        return CONFIG_READBACK_REPLACEMENTS.get(field_key, field_name)
    if field_key in ADAPTER_CONFIG_CONSTRUCTOR_ONLY_FIELDS:
        return None
    if field_name in ADAPTER_CONFIG_SECRET_FIELDS:
        return ADAPTER_CONFIG_FIELD_READBACK_REPLACEMENTS.get(field_key)
    return ADAPTER_CONFIG_FIELD_READBACK_REPLACEMENTS.get(
        field_key,
        ADAPTER_CONFIG_READBACK_REPLACEMENTS.get(field_name, field_name),
    )


def _adapter_config_field_policy(runtime_class, properties, field_key):
    module_name, class_name, field_name = field_key
    raw_property_exists = (
        field_name in properties
        or inspect.getattr_static(runtime_class, field_name, None) is not None
    )
    mismatches = []

    if field_key in ADAPTER_CONFIG_CONSTRUCTOR_ONLY_FIELDS:
        if raw_property_exists:
            mismatches.append(
                f"{module_name}.{class_name}.{field_name}: constructor-only field exposed",
            )
        readback_name = None
    elif field_name in ADAPTER_CONFIG_SECRET_FIELDS:
        if raw_property_exists:
            mismatches.append(
                f"{module_name}.{class_name}.{field_name}: raw secret property exposed",
            )
        readback_name = _config_readback_name(field_key)
    else:
        readback_name = _config_readback_name(field_key)

        if readback_name != field_name and raw_property_exists:
            mismatches.append(
                f"{module_name}.{class_name}.{field_name}: "
                f"raw property exposed alongside {readback_name}",
            )

    return readback_name, mismatches


def _config_readback_descriptor_mismatches(
    runtime_class,
    properties,
    field_key,
    readback_name,
):
    module_name, class_name, field_name = field_key
    if readback_name not in properties:
        return [f"{module_name}.{class_name}.{field_name}: missing property {readback_name}"]

    descriptor = inspect.getattr_static(runtime_class, readback_name, None)
    if descriptor is None:
        return [f"{module_name}.{class_name}.{readback_name}: missing at runtime"]
    if not inspect.isdatadescriptor(descriptor):
        return [f"{module_name}.{class_name}.{readback_name}: not a data descriptor"]
    if callable(descriptor):
        return [f"{module_name}.{class_name}.{readback_name}: exposed as method"]
    return []


def test_supported_config_py_new_and_getters_match_rust_fields():
    configs = list(_iter_supported_stub_configs(adapter=None))
    assert {(module_name, stub_class.name) for module_name, stub_class in configs} == {
        (module_name, stub_class.name)
        for adapter in (False, True)
        for module_name, stub_class in _iter_supported_stub_configs(adapter)
    }
    source_inventory = _rust_config_source_inventory(
        {stub_class.name for _, stub_class in configs},
    )
    mismatches = []

    for module_name, stub_class in configs:
        label = f"{module_name}.{stub_class.name}"
        source = source_inventory[stub_class.name]
        stub_params = _stub_constructor_param_names(stub_class)
        constructor_params = source["constructor_params"]
        if source["struct_count"] != 1:
            mismatches.append(f"{label}: Rust struct count {source['struct_count']}")
            continue
        if stub_params != constructor_params:
            mismatches.append(
                f"{label}: stub/PyO3 constructor mismatch "
                f"stub={sorted(stub_params)}, source={sorted(constructor_params)}",
            )

        source_readbacks = 0

        for field_name in sorted(stub_params):
            readback_name = _config_readback_name((module_name, stub_class.name, field_name))
            if readback_name is None:
                continue

            if readback_name not in source["getters"]:
                mismatches.append(f"{label}.{field_name}: missing PyO3 getter {readback_name}")
            else:
                source_readbacks += 1

        if (
            not source["struct_fields"]
            or not stub_params
            or not constructor_params
            or not source["struct_fields"] & constructor_params
            or source_readbacks == 0
        ):
            mismatches.append(
                f"{label}: vacuous parity fields={len(source['struct_fields'])}, "
                f"constructor={len(constructor_params)}, "
                f"source readbacks={source_readbacks}",
            )

    assert mismatches == [], "Rust/PyO3 config parity drift:\n" + "\n".join(mismatches)


def test_pyo3_constructor_params_stop_at_new_function():
    impl_block = """
        #[new]
        fn py_new(config: String) -> Self {
            Self { config }
        }

        #[pyo3(signature = (other=None))]
        fn later(other: Option<String>) {}
    """

    assert _pyo3_constructor_param_names([impl_block]) == {"config"}


def test_pyo3_getter_names_handle_supported_name_forms():
    impl_block = """
        #[getter("quoted")]
        fn py_quoted(&self) -> String {
            String::new()
        }

        #[getter]
        /// The word fn must not truncate the attribute scan.
        #[pyo3(name = "renamed")]
        fn py_renamed(&self) -> String {
            String::new()
        }

        #[getter]
        fn get_value(&self) -> String {
            String::new()
        }
    """

    assert _pyo3_getter_names(impl_block) == {"quoted", "renamed", "value"}


def test_rust_function_signature_skips_visibility_parentheses():
    content = "pub(crate) fn py_new(value: String) -> Self { Self { value } }"

    assert _rust_function_signature_after_position(content, 0) == (
        "py_new",
        "value: String",
        0,
    )


def _config_constructor_fixups_for_stub(
    content: str,
    rust_fixups: dict[str, generate_stubs.ClassMethodFixup],
) -> dict[str, generate_stubs.ClassMethodFixup]:
    config_class_names = set(STUB_CONFIG_CLASS_RE.findall(content))
    config_fixups = {}

    for rust_class, fixup in rust_fixups.items():
        python_class = fixup.python_name or rust_class
        init_defaults = fixup.signature_defaults.get("__init__")
        if python_class not in config_class_names or init_defaults is None:
            continue

        config_fixups[rust_class] = generate_stubs.ClassMethodFixup(
            python_name=fixup.python_name,
            signature_defaults={"__init__": init_defaults},
        )

    return config_fixups


def _rust_config_source_inventory(config_names):
    (
        struct_blocks,
        impl_blocks,
        getter_macro_blocks,
        macro_blocks,
        macro_invocations,
    ) = _collect_rust_config_source_blocks(config_names)
    inventory = {}

    for class_name in config_names:
        structs = struct_blocks.get(class_name, [])
        rust_fields = set().union(
            *(set(RUST_STRUCT_FIELD_RE.findall(block)) for block in structs),
        )
        class_impls = impl_blocks.get(class_name, [])
        getters = set()

        for block in class_impls:
            getters.update(_pyo3_getter_names(block))
        for block in getter_macro_blocks.get(class_name, []):
            getters.update(RUST_STRUCT_FIELD_RE.findall(block))
        for macro_key in macro_invocations.get(class_name, []):
            macro_block = macro_blocks.get(macro_key)
            if macro_block is not None:
                getters.update(_pyo3_getter_names(macro_block))

        inventory[class_name] = {
            "struct_count": len(structs),
            "struct_fields": rust_fields,
            "constructor_params": _pyo3_constructor_param_names(class_impls),
            "getters": getters,
        }

    return inventory


# Each branch indexes a distinct Rust form during the same file pass
def _collect_rust_config_source_blocks(config_names):  # noqa: C901
    struct_blocks = {}
    impl_blocks = {}
    getter_macro_blocks = {}
    macro_blocks = {}
    macro_invocations = {}

    for rust_file in sorted(WORKSPACE_ROOT.glob("crates/**/src/**/*.rs")):
        content = rust_file.read_text(encoding="utf-8")
        for match in RUST_CONFIG_STRUCT_RE.finditer(content):
            class_name = match.group(1)
            if class_name in config_names:
                struct_blocks.setdefault(class_name, []).append(
                    _rust_block_after_position(content, match.start()),
                )
        for match in RUST_CONFIG_IMPL_RE.finditer(content):
            class_name = match.group(1)
            if class_name in config_names:
                impl_blocks.setdefault(class_name, []).append(
                    _rust_block_after_position(content, match.start()),
                )
        for match in RUST_CONFIG_GETTER_MACRO_RE.finditer(content):
            class_name = match.group(1)
            if class_name in config_names:
                getter_macro_blocks.setdefault(class_name, []).append(
                    _rust_block_after_position(content, match.start()),
                )
        for match in RUST_MACRO_RE.finditer(content):
            macro_blocks[(rust_file, match.group(1))] = _rust_block_after_position(
                content,
                match.start(),
            )
        for match in RUST_CONFIG_MACRO_INVOCATION_RE.finditer(content):
            macro_name, class_name = match.groups()
            if class_name in config_names:
                macro_invocations.setdefault(class_name, []).append(
                    (rust_file, macro_name),
                )

    return (
        struct_blocks,
        impl_blocks,
        getter_macro_blocks,
        macro_blocks,
        macro_invocations,
    )


def _rust_block_after_position(content, start):
    open_brace = content.index("{", start)
    depth = 0

    for pos in range(open_brace, len(content)):
        if content[pos] == "{":
            depth += 1
        elif content[pos] == "}":
            depth -= 1
            if depth == 0:
                return content[open_brace + 1 : pos]

    raise AssertionError(f"Could not find Rust block after position {start}")


def _pyo3_constructor_param_names(impl_blocks):
    params = set()

    for block in impl_blocks:
        new_pos = block.find("#[new]")
        if new_pos < 0:
            continue
        _, arguments, function_start = _rust_function_signature_after_position(block, new_pos)
        signature = PYO3_SIGNATURE_RE.search(block, new_pos, function_start)
        if signature is not None:
            params.update(
                name for name, _ in generate_stubs._resolve_signature_params(signature.group(1))
            )
            continue

        params.update(_rust_argument_names(arguments))

    return params


def _rust_argument_names(arguments):
    names = set()

    for argument in generate_stubs._split_signature_params(arguments):
        name, separator, rust_type = argument.partition(":")
        name = name.strip().removeprefix("mut ").strip()

        if (
            not separator
            or not name
            or name.endswith("self")
            or name.startswith("_")
            or rust_type.strip().startswith("Python<")
        ):
            continue
        names.add(name)

    return names


def _pyo3_getter_names(content):
    getters = set()

    pattern = r'#\[getter(?:\((?:"([^"]+)"|([A-Za-z_]\w*))\))?\]'
    for match in re.finditer(pattern, content):
        function_name, _, function_start = _rust_function_signature_after_position(
            content,
            match.end(),
        )
        attributes = content[match.end() : function_start]
        renamed = re.search(r'#\[pyo3\(\s*name\s*=\s*"([^"]+)"', attributes)
        public_name = renamed.group(1) if renamed is not None else match.group(1) or match.group(2)
        if public_name is None:
            public_name = function_name.removeprefix("get_")
        getters.add(public_name)

    return getters


def _rust_function_signature_after_position(content, start):
    match = RUST_FUNCTION_RE.search(content, start)
    if match is None:
        raise AssertionError(f"Could not find Rust function after position {start}")

    open_paren = match.end() - 1
    depth = 0
    close_paren = None

    for pos in range(open_paren, len(content)):
        if content[pos] == "(":
            depth += 1
        elif content[pos] == ")":
            depth -= 1
            if depth == 0:
                close_paren = pos
                break
    if close_paren is None:
        raise AssertionError(f"Could not find Rust function arguments after position {start}")

    return match.group(1), content[open_paren + 1 : close_paren], match.start()


def _stub_constructor_param_names(stub_class):
    constructor = next(
        node
        for node in stub_class.body
        if isinstance(node, ast.FunctionDef) and node.name in {"__init__", "__new__"}
    )
    positional_parameters = [*constructor.args.posonlyargs, *constructor.args.args]
    return {
        argument.arg
        for argument in [*positional_parameters[1:], *constructor.args.kwonlyargs]
        if not argument.arg.startswith("_")
    }


def test_package_stub_exports_portfolio_module():
    package_stub = (STUB_ROOT / "__init__.pyi").read_text()

    assert "from . import portfolio" in package_stub
    assert '"portfolio"' in package_stub


def test_stub_enum_variants_match_screaming_snake_case():
    """
    Verify every renamed enum in .pyi stubs uses SCREAMING_SNAKE_CASE variants.

    Some variants have per-variant name overrides for letter-digit boundaries (e.g.
    LEVEL1), so we check the naming pattern rather than the exact heck conversion.

    """
    renamed_enums = generate_stubs.collect_renamed_enums(WORKSPACE_ROOT)
    stub_enums = _parse_stub_enum_variants(STUB_ROOT)

    violations = [
        f"{enum_name}.{variant}"
        for enum_name in sorted(renamed_enums)
        for variant in stub_enums.get(enum_name, [])
        if not SCREAMING_SNAKE_RE.match(variant)
    ]

    assert not violations, "Stub enum variants not in SCREAMING_SNAKE_CASE:\n" + "\n".join(
        f"  {v}" for v in violations
    )


def test_subclassable_pyclasses_are_not_final_in_stubs():
    # Arrange
    fixups = generate_stubs.collect_rust_class_fixups(WORKSPACE_ROOT)
    subclassable = {
        name
        for rust_name, fixup in fixups.items()
        if fixup.subclass
        for name in {rust_name, fixup.python_name or rust_name}
    }
    violations: list[str] = []

    # Act
    for pyi in sorted(STUB_ROOT.rglob("*.pyi")):
        lines = pyi.read_text().splitlines()
        for index, line in enumerate(lines[:-1]):
            if line.strip() != "@typing.final":
                continue

            class_match = generate_stubs.STUB_CLASS_RE.match(lines[index + 1].strip())
            if class_match is None or class_match.group(1) not in subclassable:
                continue

            relative_path = pyi.relative_to(WORKSPACE_ROOT)
            violations.append(f"{relative_path}:{index + 1}:{class_match.group(1)}")

    # Assert
    assert not violations, "Subclassable pyclasses marked final:\n" + "\n".join(
        f"  {violation}" for violation in violations
    )


def test_stub_enum_variants_match_runtime():
    """
    Verify .pyi stub enum members match the importable runtime enum members.
    """
    renamed_enums = generate_stubs.collect_renamed_enums(WORKSPACE_ROOT)
    stub_enums = _parse_stub_enum_variants(STUB_ROOT)
    runtime_enums = _collect_runtime_enum_variants(STUB_ROOT)

    mismatches: list[str] = []

    for name, runtime_members in sorted(runtime_enums.items()):
        expected_runtime_members = runtime_members

        if name in renamed_enums:
            expected_runtime_members = [
                generate_stubs.to_screaming_snake_case(variant) for variant in runtime_members
            ]

        stub_members = stub_enums.get(name)
        if stub_members is None:
            continue

        if set(expected_runtime_members) != set(stub_members):
            runtime_only = set(expected_runtime_members) - set(stub_members)
            stub_only = set(stub_members) - set(expected_runtime_members)
            parts = [name]

            if runtime_only:
                parts.append(f"runtime only: {sorted(runtime_only)}")
            if stub_only:
                parts.append(f"stub only: {sorted(stub_only)}")
            mismatches.append(" | ".join(parts))

    assert not mismatches, "Stub/runtime enum member mismatches:\n" + "\n".join(
        f"  {m}" for m in mismatches
    )


def _collect_runtime_enum_variants(stub_root: Path) -> dict[str, list[str]]:
    result: dict[str, list[str]] = {}

    for module in _iter_public_runtime_modules(stub_root):
        for name in sorted(dir(module)):
            obj = getattr(module, name)
            if not (isinstance(obj, type) and hasattr(obj, "variants")):
                continue

            try:
                result[name] = [variant.name for variant in obj.variants()]
            except Exception:  # noqa: S112
                continue

    if not result:
        pytest.skip("No importable runtime enum modules available")

    return result


def _iter_public_runtime_modules(stub_root: Path):
    for stub_path in sorted(stub_root.rglob("__init__.pyi")):
        relative_package = stub_path.relative_to(stub_root).parent
        if any(part.startswith("_") for part in relative_package.parts):
            continue

        module_name = _module_name_from_stub_path(relative_package)
        try:
            yield importlib.import_module(module_name)
        except ImportError:
            continue


def _module_name_from_stub_path(relative_package: Path) -> str:
    if not relative_package.parts:
        return "nautilus_trader"

    return f"nautilus_trader.{'.'.join(relative_package.parts)}"

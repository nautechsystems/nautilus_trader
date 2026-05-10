from __future__ import annotations

import importlib
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

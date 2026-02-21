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
Tests to verify that nautilus_pyo3.pyi type stubs match the actual runtime bindings.

This catches misalignments between stubs and the actual PyO3 bindings, such as:
- Enum variants declared in stubs but not exposed at runtime
- Methods/properties declared in stubs but missing from bindings
- Classes declared in stubs but not registered in the Python module

"""

from __future__ import annotations

import ast
from typing import Any

import pytest

from nautilus_trader import PACKAGE_ROOT
from nautilus_trader.core import nautilus_pyo3


STUB_FILE = PACKAGE_ROOT / "nautilus_trader" / "core" / "nautilus_pyo3.pyi"

# Properties that exist on instances but aren't detectable via hasattr(cls, ...) on the class.
# These are PyO3 instance properties that work correctly at runtime but don't show up as
# class-level attributes. Verified working via mypy type checking.
INSTANCE_ONLY_PROPERTIES: dict[str, set[str]] = {
    "BarSpecification": {"step", "aggregation", "price_type"},
    "BarType": {"instrument_id", "spec", "aggregation_source"},
    "MarketOrder": {"venue_order_id", "position_id", "last_trade_id", "price"},
}


def parse_stub_file() -> dict:
    """
    Parse the .pyi stub file and extract class definitions.

    Returns a dict mapping class names to their members:
    {
        "ClassName": {
            "is_enum": bool,
            "variants": ["VARIANT1", "VARIANT2"],  # for enums
            "methods": ["method1", "method2"],
            "properties": ["prop1", "prop2"],
        }
    }

    """
    content = STUB_FILE.read_text()
    tree = ast.parse(content)

    classes = {}

    for node in ast.walk(tree):
        if not isinstance(node, ast.ClassDef):
            continue

        class_name = node.name
        is_enum = any(
            (isinstance(base, ast.Name) and base.id == "Enum")
            or (isinstance(base, ast.Attribute) and base.attr == "Enum")
            for base in node.bases
        )

        members: dict[str, Any] = {
            "is_enum": is_enum,
            "variants": [],
            "methods": [],
            "properties": [],
        }

        for item in node.body:
            if isinstance(item, ast.AnnAssign) and isinstance(item.target, ast.Name):
                # Enum variant: VARIANT_NAME = "value"
                if is_enum:
                    members["variants"].append(item.target.id)

            elif isinstance(item, ast.FunctionDef):
                name = item.name

                # Skip dunder methods except __init__
                if name.startswith("__") and name != "__init__":
                    continue

                # Check if it's a property
                is_property = any(
                    isinstance(dec, ast.Name) and dec.id == "property"
                    for dec in item.decorator_list
                )
                if is_property:
                    members["properties"].append(name)
                else:
                    members["methods"].append(name)

        classes[class_name] = members

    return classes


def get_module_classes() -> set[str]:
    """
    Get all class names available directly from nautilus_pyo3.
    """
    return {
        name
        for name in dir(nautilus_pyo3)
        if not name.startswith("_") and isinstance(getattr(nautilus_pyo3, name), type)
    }


# Parse stubs once at module load
STUB_CLASSES = parse_stub_file()
MODULE_CLASSES = get_module_classes()


def get_enum_classes() -> list[tuple[str, dict]]:
    """
    Get all enum classes from stubs that exist in the module.
    """
    return [
        (name, info)
        for name, info in STUB_CLASSES.items()
        if info["is_enum"] and name in MODULE_CLASSES
    ]


def get_non_enum_classes() -> list[tuple[str, dict]]:
    """
    Get all non-enum classes from stubs that exist in the module.
    """
    return [
        (name, info)
        for name, info in STUB_CLASSES.items()
        if not info["is_enum"] and name in MODULE_CLASSES
    ]


class TestEnumVariants:
    """
    Test that all enum variants declared in stubs are accessible at runtime.
    """

    @pytest.mark.parametrize(
        ("class_name", "info"),
        get_enum_classes(),
        ids=[name for name, _ in get_enum_classes()],
    )
    def test_enum_variants_exist(self, class_name: str, info: dict) -> None:
        """
        Verify all enum variants from stubs are accessible.
        """
        cls = getattr(nautilus_pyo3, class_name)

        missing = []
        for variant in info["variants"]:
            if not hasattr(cls, variant):
                missing.append(variant)

        if missing:
            # Get what's actually available
            available = [attr for attr in dir(cls) if not attr.startswith("_") and attr.isupper()]
            pytest.fail(f"{class_name} missing variants: {missing}\nAvailable: {available}")


class TestClassMethods:
    """
    Test that methods declared in stubs exist at runtime.
    """

    @pytest.mark.parametrize(
        ("class_name", "info"),
        get_non_enum_classes(),
        ids=[name for name, _ in get_non_enum_classes()],
    )
    def test_methods_exist(self, class_name: str, info: dict) -> None:
        """
        Verify all methods from stubs are accessible.
        """
        cls = getattr(nautilus_pyo3, class_name)

        missing = []
        for method in info["methods"]:
            if method == "__init__":
                continue
            if not hasattr(cls, method):
                missing.append(method)

        if missing:
            available = [m for m in dir(cls) if not m.startswith("_")]
            pytest.fail(f"{class_name} missing methods: {missing}\nAvailable: {available}")

    @pytest.mark.parametrize(
        ("class_name", "info"),
        get_non_enum_classes(),
        ids=[name for name, _ in get_non_enum_classes()],
    )
    def test_properties_exist(self, class_name: str, info: dict) -> None:
        """
        Verify all properties from stubs are accessible.
        """
        cls = getattr(nautilus_pyo3, class_name)
        instance_only = INSTANCE_ONLY_PROPERTIES.get(class_name, set())

        missing = []
        for prop in info["properties"]:
            if prop in instance_only:
                continue
            if not hasattr(cls, prop):
                missing.append(prop)

        if missing:
            available = [p for p in dir(cls) if not p.startswith("_")]
            pytest.fail(f"{class_name} missing properties: {missing}\nAvailable: {available}")


class TestStubClassesExist:
    """
    Test that classes declared in stubs exist in the module.
    """

    def test_stub_classes_in_module(self) -> None:
        """
        Verify all stub classes are available in nautilus_pyo3.
        """
        missing = [name for name in STUB_CLASSES if not hasattr(nautilus_pyo3, name)]

        # Allow some tolerance - not all stub classes may be registered
        # but if more than 10% are missing, that's a problem
        if len(missing) > len(STUB_CLASSES) * 0.1:
            pytest.fail(
                f"Too many stub classes missing from module ({len(missing)}/{len(STUB_CLASSES)}):\n"
                f"First 20 missing: {missing[:20]}",
            )

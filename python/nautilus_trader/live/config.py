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
Importable configuration for Python adapter clients.
"""

from __future__ import annotations

import importlib
import json
from dataclasses import dataclass
from dataclasses import field
from decimal import Decimal
from decimal import InvalidOperation
from types import GetSetDescriptorType
from types import UnionType
from typing import Any
from typing import ClassVar
from typing import Union
from typing import get_args
from typing import get_origin
from typing import get_type_hints

from nautilus_trader._libnautilus.live import InstrumentProviderConfig
from nautilus_trader._libnautilus.live import RoutingConfig


@dataclass(frozen=True)
class ImportableFactoryConfig:
    """
    A factory import path in ``module:qualified_name`` form.
    """

    path: str

    def create(self) -> object:
        """
        Import and construct the configured factory.
        """
        return resolve_path(self.path)()


@dataclass(frozen=True)
class ImportableConfig:
    """
    An importable client configuration and optional factory descriptor.
    """

    path: str
    config: dict = field(default_factory=dict)
    factory: ImportableFactoryConfig | None = None

    def create(self) -> object:
        """
        Construct the original configuration class with all recorded fields.
        """
        return construct_config(resolve_path(self.path), self.config)

    def dict(self) -> dict[str, object]:
        """
        Return the descriptor fields without converting their Python values.
        """
        return {
            "path": self.path,
            "config": self.config.copy(),
            "factory": {"path": self.factory.path} if self.factory is not None else None,
        }

    def json(self) -> bytes:
        """
        Encode the descriptor as JSON bytes, rejecting unsupported values.
        """
        return json.dumps(self.dict(), default=encode_config_value, allow_nan=False).encode()

    @classmethod
    def parse(cls, raw: str | bytes) -> ImportableConfig:
        """
        Decode a JSON descriptor while retaining all configuration fields.
        """
        values = json.loads(raw)
        if values.get("factory") is not None:
            values["factory"] = ImportableFactoryConfig(**values["factory"])
        return cls(**values)


def resolve_path(path: str) -> object:
    """
    Resolve a module and qualified class name.
    """
    module, separator, name = path.partition(":")
    if not separator or not module or not name or "<locals>" in name:
        raise ValueError("Import path must have the form module:qualified_name")
    value = importlib.import_module(module)
    for part in name.split("."):
        value = getattr(value, part)
    return value


def config_values(config: object) -> dict[str, object]:
    """
    Return instance config fields without class metadata.
    """
    names = set(vars(config)) if hasattr(config, "__dict__") else set()
    for cls in type(config).__mro__:
        names.update(getattr(cls, "__annotations__", ()))
        names.update(
            name for name, value in vars(cls).items() if isinstance(value, GetSetDescriptorType)
        )
        slots = vars(cls).get("__slots__", ())
        names.update((slots,) if isinstance(slots, str) else slots)
    class_fields = {
        name for name, hint in get_type_hints(type(config)).items() if get_origin(hint) is ClassVar
    }
    return {
        name: getattr(config, name)
        for name in sorted(names - class_fields)
        if not name.startswith("__")
    }


def config_json(config: object) -> bytes:
    """
    Serialize configuration fields to JSON bytes.
    """
    return json.dumps(config_values(config), default=encode_config_value, allow_nan=False).encode()


def config_importable(
    config: object,
    factory: ImportableFactoryConfig | None = None,
) -> ImportableConfig:
    """
    Describe an importable config while preserving its concrete class.
    """
    cls = type(config)
    path = f"{cls.__module__}:{cls.__qualname__}"
    if resolve_path(path) is not cls:
        raise ValueError("Configuration class is not importable by its qualified name")
    return ImportableConfig(path, config_values(config), factory)


def encode_config_value(value: object) -> object:
    """
    Encode supported domain values for JSON configuration.
    """
    if isinstance(value, (RoutingConfig, InstrumentProviderConfig)):
        return config_values(value)
    if isinstance(value, Decimal):
        return str(value)
    if callable(getattr(type(value), "from_str", None)):
        return str(value)
    raise TypeError(f"Unsupported configuration value type: {type(value).__qualname__}")


def construct_config(cls: type, values: dict[str, object]) -> object:
    """
    Restore typed fields before constructing the config class.
    """
    values = dict(values)

    for name, native_type in (
        ("routing", RoutingConfig),
        ("instrument_provider", InstrumentProviderConfig),
    ):
        if isinstance(values.get(name), dict):
            values[name] = native_type(**values[name])
    for name, hint in get_type_hints(cls).items():
        if name in values:
            values[name] = decode_config_value(hint, values[name])
    return cls(**values)


def decode_config_value(hint: object, value: object) -> object:  # noqa: PLR0911 - Keep the complete typed dispatch or engine scenario together.
    """
    Restore a JSON value according to its declared field type.
    """
    if hint is Any:
        return value
    origin = get_origin(hint)
    args = get_args(hint)
    if origin in (Union, UnionType):
        for member in args:
            try:
                return decode_config_value(member, value)
            except (TypeError, ValueError, InvalidOperation):
                pass
        raise TypeError(f"Configuration value does not match {hint}")
    if origin is list and isinstance(value, list):
        return [decode_config_value(args[0], item) for item in value]
    if origin is dict and isinstance(value, dict):
        return {
            decode_config_value(args[0], key): decode_config_value(args[1], item)
            for key, item in value.items()
        }
    if isinstance(hint, type) and isinstance(value, hint):
        return value
    if hint is Decimal and isinstance(value, str):
        return Decimal(value)
    if isinstance(value, str) and callable(getattr(hint, "from_str", None)):
        return hint.from_str(value)
    raise TypeError(f"Unsupported configuration value for {hint}")


def resolve_client_registration(factory: object, config: object) -> tuple[object, object]:
    """
    Resolve importable descriptors before client registration.
    """
    if isinstance(config, ImportableConfig):
        factory = factory if factory is not None else config.factory
        config = config.create()
    if isinstance(factory, ImportableFactoryConfig):
        factory = factory.create()
    return factory, config


def configured_clients(
    config: object,
    factories: dict[str, object] | None,
) -> list[tuple[str, object, object]]:
    """
    Resolve named factories with explicit names taking precedence.
    """
    factories = factories or {}
    registrations = []

    for name, client_config in config.items():
        factory = factories.get(name, factories.get(name.split("-", 1)[0]))
        factory, resolved_config = resolve_client_registration(factory, client_config)
        if factory is None:
            raise ValueError(f"No factory registered for client '{name}'")
        registrations.append((name, factory, resolved_config))
    return registrations

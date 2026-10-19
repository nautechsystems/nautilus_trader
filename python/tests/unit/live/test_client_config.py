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
Verify Python adapter configuration and live engine integration.
"""

import sys
from decimal import Decimal
from typing import Any
from typing import ClassVar

import pytest

from nautilus_trader.config import DataClientConfig
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import RoutingConfig
from nautilus_trader.live.config import ImportableConfig
from nautilus_trader.live.config import ImportableFactoryConfig
from nautilus_trader.live.config import config_importable
from nautilus_trader.live.config import config_json
from nautilus_trader.live.config import config_values
from nautilus_trader.live.config import configured_clients
from nautilus_trader.live.config import decode_config_value
from nautilus_trader.model import InstrumentId


class Config(DataClientConfig):
    """
    Retain adapter fields alongside the native configuration.
    """

    label: str
    amount: Decimal
    instruments: list[InstrumentId]

    def __init__(self, *, label, amount, instruments, **_kwargs: object) -> None:
        """
        Retain the component inputs without starting asynchronous work.
        """
        self.label = label
        self.amount = amount
        self.instruments = instruments


class Factory:
    """
    Construct clients for the deterministic live scenario.
    """


class AnnotatedConfig(DataClientConfig):
    """
    Retain adapter fields alongside the native configuration.
    """

    kind: ClassVar[str] = "annotated"
    _label: str
    options: dict[str, Any]

    def __init__(self, *, _label, options, **_kwargs: object) -> None:
        """
        Retain the component inputs without starting asynchronous work.
        """
        self._label = _label
        self.options = options


def test_importable_config_round_trip_preserves_subclass_and_common_fields() -> None:
    """
    Importable config round trip preserves subclass and common fields.
    """
    config = Config(
        label="external",
        amount=Decimal("1.234567890123456789"),
        instruments=[InstrumentId.from_str("EUR/USD.SIM")],
        handle_revised_bars=True,
        routing=RoutingConfig(default=True, venues=["SIM", "EXTERNAL"]),
        instrument_provider=InstrumentProviderConfig(load_all=True, filters={"status": "active"}),
    )
    factory = ImportableFactoryConfig(f"{__name__}:Factory")
    descriptor = config.to_importable(factory)
    decoded = ImportableConfig.parse(descriptor.json())
    restored = decoded.create()

    assert type(restored) is Config
    assert restored.label == "external"
    assert restored.amount == Decimal("1.234567890123456789")
    assert restored.instruments == [InstrumentId.from_str("EUR/USD.SIM")]
    assert restored.handle_revised_bars is True
    assert restored.routing.default is True
    assert restored.routing.venues == ["SIM", "EXTERNAL"]
    assert restored.instrument_provider.load_all is True
    assert restored.instrument_provider.filters == {"status": "active"}
    assert decoded.factory == factory
    assert type(decoded.factory.create()) is Factory
    assert restored.json() == config.json()
    assert restored.dict()["label"] == "external"
    assert config_values(restored)["amount"] == config.amount


@pytest.mark.parametrize("exact", [False, True])
def test_named_factory_uses_exact_name_before_prefix(exact) -> None:
    """
    Choose an exact factory when present, otherwise use the venue prefix.
    """
    prefix = object()
    named = object()
    config = Config(label="routing", amount=Decimal(13), instruments=[])
    factories = {"VENUE": prefix}
    if exact:
        factories["VENUE-1"] = named

    registrations = configured_clients({"VENUE-1": config}, factories)

    assert registrations == [("VENUE-1", named if exact else prefix, config)]


@pytest.mark.parametrize("explicit", [False, True])
def test_explicit_factory_precedes_importable_config_factory(explicit) -> None:
    """
    Use the embedded factory only when no explicit factory is registered.
    """
    config = Config(
        label="factory precedence",
        amount=Decimal("17.125"),
        instruments=[InstrumentId.from_str("EUR/USD.SIM")],
    )
    descriptor = config.to_importable(ImportableFactoryConfig(f"{__name__}:Factory"))
    selected = object()

    registrations = configured_clients(
        {"VENUE-1": descriptor},
        {"VENUE-1": selected} if explicit else {},
    )

    assert len(registrations) == 1
    name, factory, restored = registrations[0]
    assert name == "VENUE-1"

    if explicit:
        assert factory is selected
    else:
        assert type(factory) is Factory
    assert type(restored) is Config
    assert restored.json() == config.json()


def test_config_serialization_rejects_unsupported_values() -> None:
    """
    Config serialization rejects unsupported values.
    """
    config = Config(label="unsupported", amount=Decimal(3), instruments=[])
    config.extra = object()
    with pytest.raises(TypeError, match="Unsupported configuration value type: object"):
        config_json(config)


def test_importable_config_rejects_local_classes() -> None:
    """
    Importable config rejects local classes.
    """

    class Local(Config):
        pass

    config = Local(label="local", amount=Decimal(5), instruments=[])
    with pytest.raises(ValueError, match="module:qualified_name"):
        config_importable(config)


def test_config_round_trip_retains_private_fields_and_ignores_class_metadata() -> None:
    """
    Config round trip retains private fields and ignores class metadata.
    """
    config = AnnotatedConfig(_label="private field", options={"mode": "paper", "retries": 3})
    restored = ImportableConfig.parse(config.to_importable().json()).create()

    assert type(restored) is AnnotatedConfig
    assert restored._label == "private field"
    assert restored.options == {"mode": "paper", "retries": 3}
    assert restored.kind == "annotated"
    assert "kind" not in restored.dict()
    assert restored.json() == config.json()


@pytest.mark.parametrize(
    "path",
    ["", "missing_separator", ":Config", "module:", "module:outer.<locals>.Config"],
)
def test_import_path_rejects_malformed_names(path) -> None:
    """
    Malformed paths fail before importing a module.
    """
    with pytest.raises(ValueError, match="module:qualified_name"):
        ImportableConfig(path).create()


@pytest.mark.parametrize("factories", [None, {}, {"OTHER": Factory()}])
def test_configured_client_requires_matching_factory(factories) -> None:
    """
    A missing registration names the client which cannot be constructed.
    """
    with pytest.raises(ValueError, match="No factory registered for client 'VENUE-2'"):
        configured_clients({"VENUE-2": DataClientConfig()}, factories)


@pytest.mark.parametrize(
    ("hint", "value", "expected"),
    [
        (Decimal | str, "paper", "paper"),
        (int | None, None, None),
        (int | None, 17, 17),
        (int | Decimal, "1.234567890123456789", Decimal("1.234567890123456789")),
        (list[Decimal], ["1.25", "3.75"], [Decimal("1.25"), Decimal("3.75")]),
        (
            dict[InstrumentId, Decimal],
            {"EUR/USD.SIM": "9.125"},
            {InstrumentId.from_str("EUR/USD.SIM"): Decimal("9.125")},
        ),
    ],
)
def test_config_decode_preserves_declared_nested_types(hint, value, expected) -> None:
    """
    Typed decoding preserves exact values after union fallback.
    """
    assert decode_config_value(hint, value) == expected


@pytest.mark.parametrize(
    ("hint", "value"),
    [
        (int | None, "17"),
        (Decimal | int, "paper"),
        (list[int], {}),
        (dict[str, int], []),
        (list[int], ["17"]),
        (Decimal, 1.25),
    ],
)
def test_config_decode_rejects_incompatible_values(hint, value) -> None:
    """
    Invalid union members and container shapes cannot silently coerce.
    """
    with pytest.raises(
        TypeError,
        match=r"Configuration value does not match|Unsupported configuration value",
    ):
        decode_config_value(hint, value)


def test_config_importable_rejects_replaced_class_identity(monkeypatch) -> None:
    """
    A valid import path must still resolve to the concrete config class.
    """
    config = Config(label="original", amount=Decimal(3), instruments=[])
    monkeypatch.setattr(sys.modules[__name__], "Config", AnnotatedConfig)
    with pytest.raises(ValueError, match="not importable by its qualified name"):
        config_importable(config)


@pytest.mark.parametrize("slots", ["label", ("label",)])
def test_config_values_supports_string_and_tuple_slots(slots) -> None:
    """
    Slotted fields survive serialization without class metadata.
    """
    cls = type("SlottedConfig", (), {"__slots__": slots})
    config = cls()
    config.label = "slotted"

    assert config_values(config) == {"label": "slotted"}

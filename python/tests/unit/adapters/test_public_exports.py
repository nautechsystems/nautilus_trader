# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Naute Systems Pty Ltd. All rights reserved.
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
Test public exports behavior.
"""

from __future__ import annotations

import ast
import importlib
from pathlib import Path

import pytest

from nautilus_trader.model import Venue


ADAPTERS_ROOT = Path(__file__).resolve().parents[3] / "nautilus_trader" / "adapters"

# Every adapter package under python/nautilus_trader/adapters/.
ADAPTERS = sorted(p.name for p in ADAPTERS_ROOT.iterdir() if (p / "__init__.py").exists())

# Venue adapters expose canonical <VENUE>, <VENUE>_CLIENT_ID, <VENUE>_VENUE constants.
# Data providers (databento, tardis), the blockchain data client, the sandbox exec
# client, and the multi-venue interactive_brokers broker intentionally omit them.
VENUE_ADAPTERS = {
    "architect_ax": "AX",
    "betfair": "BETFAIR",
    "binance": "BINANCE",
    "bitmex": "BITMEX",
    "bybit": "BYBIT",
    "coinbase": "COINBASE",
    "deribit": "DERIBIT",
    "derive": "DERIVE",
    "dydx": "DYDX",
    "hyperliquid": "HYPERLIQUID",
    "kraken": "KRAKEN",
    "lighter": "LIGHTER",
    "okx": "OKX",
    "polymarket": "POLYMARKET",
}

NON_VENUE_ADAPTERS = sorted(set(ADAPTERS) - set(VENUE_ADAPTERS))

# Members that must never reach a facade's public surface: raw transport clients,
# endpoint helpers, and leaked future-import names.
FORBIDDEN_SUFFIXES = (
    "HttpClient",
    "RawHttpClient",
    "WebSocketClient",
    "GrpcClient",
    "Wallet",
    "OrderSubmitter",
    "HTTP_URL",
    "WS_URL",
)
FORBIDDEN_NAMES = {"annotations"}


def _is_forbidden(name: str) -> bool:
    if name in FORBIDDEN_NAMES:
        return True
    if name.endswith(FORBIDDEN_SUFFIXES):
        return True
    # Endpoint helpers such as get_<adapter>_http_base_url / get_<adapter>_ws_url,
    # but not legitimate utilities such as get_<adapter>_arrow_schema_map.
    return name.startswith("get_") and "url" in name


def _import(adapter: str) -> object:
    return importlib.import_module(f"nautilus_trader.adapters.{adapter}")


def _stub_all(adapter: str) -> list[str]:
    stub_path = ADAPTERS_ROOT / adapter / "__init__.pyi"
    tree = ast.parse(stub_path.read_text())
    for node in tree.body:
        if isinstance(node, ast.Assign) and any(
            isinstance(target, ast.Name) and target.id == "__all__" for target in node.targets
        ):
            return ast.literal_eval(node.value)
    pytest.fail(f"{adapter} stub has no __all__")


@pytest.mark.parametrize("adapter", ADAPTERS)
def test_adapter_all_is_unique_nonempty(adapter: object) -> None:
    """
    Test adapter all is unique nonempty.
    """
    module = _import(adapter)

    assert module.__all__, f"{adapter} __all__ must be non-empty"
    assert len(module.__all__) == len(set(module.__all__)), (
        f"{adapter} __all__ must not contain duplicates"
    )
    # Ordering determinism is enforced by the RUF022 pre-commit gate; runtime and
    # stub agreement is checked in test_runtime_all_matches_stub_all_exactly.


@pytest.mark.parametrize("adapter", ADAPTERS)
def test_adapter_all_names_resolve(adapter: object) -> None:
    """
    Test adapter all names resolve.
    """
    module = _import(adapter)

    missing = [name for name in module.__all__ if not hasattr(module, name)]
    assert not missing, f"{adapter} __all__ names missing at runtime: {missing}"


@pytest.mark.parametrize("adapter", ADAPTERS)
def test_runtime_all_matches_stub_all_exactly(adapter: object) -> None:
    """
    Test runtime all matches stub all exactly.
    """
    module = _import(adapter)

    assert list(module.__all__) == _stub_all(adapter), (
        f"{adapter} runtime __all__ and stub __all__ disagree"
    )


@pytest.mark.parametrize("adapter", ADAPTERS)
def test_facade_exposes_no_raw_clients_endpoints_or_helpers(adapter: object) -> None:
    """
    Test facade exposes no raw clients endpoints or helpers.
    """
    module = _import(adapter)

    leaked = [name for name in module.__all__ if _is_forbidden(name)]

    assert not leaked, f"{adapter} __all__ leaks private members: {leaked}"


@pytest.mark.parametrize("adapter", ADAPTERS)
def test_public_classes_owned_by_adapter_package(adapter: object) -> None:
    """
    Test public classes owned by adapter package.
    """
    module = _import(adapter)
    expected_module = f"nautilus_trader.adapters.{adapter}"

    misowned = []

    for name in module.__all__:
        obj = getattr(module, name)
        if isinstance(obj, type) and obj.__module__ != expected_module:
            misowned.append(f"{name} -> {obj.__module__}")

    assert not misowned, f"{adapter} public classes not owned by facade: {misowned}"


@pytest.mark.parametrize(("adapter", "venue"), sorted(VENUE_ADAPTERS.items()))
def test_venue_adapter_exposes_canonical_constants(adapter: object, venue: Venue) -> None:
    """
    Test venue adapter exposes canonical constants.
    """
    module = _import(adapter)

    assert module.__all__.count(venue) == 1
    assert module.__all__.count(f"{venue}_CLIENT_ID") == 1
    assert module.__all__.count(f"{venue}_VENUE") == 1
    assert getattr(module, venue) == venue


@pytest.mark.parametrize("adapter", NON_VENUE_ADAPTERS)
def test_non_venue_adapter_has_no_venue_constants(adapter: object) -> None:
    """
    Test non venue adapter has no venue constants.
    """
    module = _import(adapter)

    venue_constants = [name for name in module.__all__ if name.endswith("_VENUE")]
    assert not venue_constants, f"{adapter} must not define venue constants: {venue_constants}"


def test_known_adapter_set_is_complete() -> None:
    """
    Test known adapter set is complete.
    """
    # Guards against a new adapter landing without a deliberate facade decision.
    expected = {
        "architect_ax",
        "betfair",
        "binance",
        "bitmex",
        "blockchain",
        "bybit",
        "coinbase",
        "databento",
        "deribit",
        "derive",
        "dydx",
        "hyperliquid",
        "interactive_brokers",
        "kraken",
        "lighter",
        "okx",
        "polymarket",
        "sandbox",
        "tardis",
    }
    assert set(ADAPTERS) == expected

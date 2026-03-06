from __future__ import annotations

import pytest

from nautilus_trader.adapters.binance import BINANCE
from nautilus_trader.adapters.binance import BinanceDataClientConfig
from nautilus_trader.adapters.bybit import BYBIT
from nautilus_trader.adapters.bybit import BybitDataClientConfig
from nautilus_trader.adapters.bybit import BybitExecClientConfig
from nautilus_trader.adapters.bybit import BybitProductType
from nautilus_trader.flux.runners.live.venues import resolve_strategy_venues


def test_resolve_strategy_venues_builds_clients_from_generic_node_venues_table() -> None:
    resolved = resolve_strategy_venues(
        config={
            "venues": {
                "execution_venue": "BYBIT",
                "reference_venue": "BINANCE",
            },
            "node": {
                "venues": {
                    "BYBIT": {
                        "adapter": "bybit",
                        "instrument_id": "PLUMEUSDT-LINEAR.BYBIT",
                        "product_type": "LINEAR",
                        "recv_window_ms": 20_000,
                        "execution": True,
                    },
                    "BINANCE": {
                        "adapter": "binance",
                        "instrument_id": "PLUMEUSDT.BINANCE",
                        "account_type": "SPOT",
                    },
                },
            },
        },
        mode="paper",
        enable_execution=True,
    )

    assert resolved.execution_venue == "BYBIT"
    assert resolved.reference_venue == "BINANCE"
    assert str(resolved.execution_instrument_id) == "PLUMEUSDT-LINEAR.BYBIT"
    assert str(resolved.reference_instrument_id) == "PLUMEUSDT.BINANCE"
    assert set(resolved.data_clients) == {BYBIT, BINANCE}
    assert set(resolved.exec_clients) == {BYBIT}
    assert isinstance(resolved.data_clients[BYBIT], BybitDataClientConfig)
    assert isinstance(resolved.data_clients[BINANCE], BinanceDataClientConfig)
    assert isinstance(resolved.exec_clients[BYBIT], BybitExecClientConfig)


def test_resolve_strategy_venues_rejects_unknown_adapter() -> None:
    with pytest.raises(ValueError, match="Unsupported adapter"):
        resolve_strategy_venues(
            config={
                "venues": {
                    "execution_venue": "BYBIT",
                    "reference_venue": "BINANCE",
                },
                "node": {
                    "venues": {
                        "BYBIT": {
                            "adapter": "not-a-real-adapter",
                            "instrument_id": "PLUMEUSDT-LINEAR.BYBIT",
                        },
                        "BINANCE": {
                            "adapter": "binance",
                            "instrument_id": "PLUMEUSDT.BINANCE",
                        },
                    },
                },
            },
            mode="paper",
            enable_execution=False,
        )


def test_resolve_strategy_venues_rejects_non_positive_recv_window() -> None:
    with pytest.raises(ValueError, match="node.venues.BYBIT.recv_window_ms"):
        resolve_strategy_venues(
            config={
                "venues": {
                    "execution_venue": "BYBIT",
                    "reference_venue": "BINANCE",
                },
                "node": {
                    "venues": {
                        "BYBIT": {
                            "adapter": "bybit",
                            "instrument_id": "PLUMEUSDT-LINEAR.BYBIT",
                            "recv_window_ms": 0,
                        },
                        "BINANCE": {
                            "adapter": "binance",
                            "instrument_id": "PLUMEUSDT.BINANCE",
                        },
                    },
                },
            },
            mode="paper",
            enable_execution=False,
        )


def test_resolve_strategy_venues_supports_legacy_node_exchange_tables() -> None:
    resolved = resolve_strategy_venues(
        config={
            "venues": {
                "execution_venue": "BYBIT",
                "reference_venue": "BINANCE",
            },
            "node": {
                "maker_instrument_id": "PLUMEUSDT-LINEAR.BYBIT",
                "reference_instrument_id": "PLUMEUSDT.BINANCE",
                "bybit": {
                    "product_type": "LINEAR",
                },
                "binance": {
                    "account_type": "SPOT",
                },
            },
        },
        mode="paper",
        enable_execution=False,
    )

    assert str(resolved.execution_instrument_id) == "PLUMEUSDT-LINEAR.BYBIT"
    assert str(resolved.reference_instrument_id) == "PLUMEUSDT.BINANCE"
    assert set(resolved.data_clients) == {BYBIT, BINANCE}
    assert resolved.exec_clients == {}


def test_resolve_strategy_venues_legacy_fallback_keeps_prior_default_instruments() -> None:
    resolved = resolve_strategy_venues(
        config={
            "venues": {
                "execution_venue": "BYBIT",
                "reference_venue": "BINANCE",
            },
            "node": {
                "bybit": {},
                "binance": {},
            },
        },
        mode="paper",
        enable_execution=False,
    )

    assert str(resolved.execution_instrument_id) == "PLUMEUSDT-LINEAR.BYBIT"
    assert str(resolved.reference_instrument_id) == "PLUMEUSDT.BINANCE"


def test_resolve_strategy_venues_legacy_fallback_keeps_prior_bybit_product_default() -> None:
    resolved = resolve_strategy_venues(
        config={
            "venues": {
                "execution_venue": "BYBIT",
                "reference_venue": "BINANCE",
            },
            "node": {
                "bybit": {},
                "binance": {},
            },
        },
        mode="paper",
        enable_execution=False,
    )

    assert resolved.data_clients[BYBIT].product_types == (BybitProductType.LINEAR,)

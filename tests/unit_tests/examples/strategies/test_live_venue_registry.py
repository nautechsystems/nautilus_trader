from __future__ import annotations

import pytest

from flux.runners.live.venues import resolve_strategy_venues
from nautilus_trader.adapters.binance import BINANCE
from nautilus_trader.adapters.binance import BinanceDataClientConfig
from nautilus_trader.adapters.binance import BinanceExecClientConfig
from nautilus_trader.adapters.binance.common.enums import BinanceEnvironment
from nautilus_trader.adapters.bybit import BYBIT
from nautilus_trader.adapters.bybit import BybitDataClientConfig
from nautilus_trader.adapters.bybit import BybitExecClientConfig
from nautilus_trader.adapters.bybit import BybitProductType
from nautilus_trader.adapters.hyperliquid import HYPERLIQUID
from nautilus_trader.adapters.hyperliquid import HyperliquidDataClientConfig
from nautilus_trader.adapters.hyperliquid import HyperliquidExecClientConfig
from nautilus_trader.adapters.okx import OKX
from nautilus_trader.adapters.okx import OKXDataClientConfig
from nautilus_trader.adapters.okx import OKXExecClientConfig


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


def test_resolve_strategy_venues_supports_binance_spot_reference_and_binance_perp_execution() -> (
    None
):
    resolved = resolve_strategy_venues(
        config={
            "venues": {
                "execution_venue": "BINANCE_PERP",
                "reference_venue": "BINANCE_SPOT",
            },
            "node": {
                "venues": {
                    "BINANCE_PERP": {
                        "adapter": "binance",
                        "instrument_id": "PLUMEUSDT-PERP.BINANCE_PERP",
                        "account_type": "USDT_FUTURES",
                        "execution": True,
                    },
                    "BINANCE_SPOT": {
                        "adapter": "binance",
                        "instrument_id": "PLUMEUSDT.BINANCE_SPOT",
                        "account_type": "SPOT",
                    },
                },
            },
        },
        mode="paper",
        enable_execution=True,
    )

    assert resolved.execution_venue == "BINANCE_PERP"
    assert resolved.reference_venue == "BINANCE_SPOT"
    assert str(resolved.execution_instrument_id) == "PLUMEUSDT-PERP.BINANCE_PERP"
    assert str(resolved.reference_instrument_id) == "PLUMEUSDT.BINANCE_SPOT"
    assert set(resolved.data_clients) == {"BINANCE_PERP", "BINANCE_SPOT"}
    assert set(resolved.exec_clients) == {"BINANCE_PERP"}
    assert isinstance(resolved.data_clients["BINANCE_PERP"], BinanceDataClientConfig)
    assert isinstance(resolved.data_clients["BINANCE_SPOT"], BinanceDataClientConfig)
    assert isinstance(resolved.exec_clients["BINANCE_PERP"], BinanceExecClientConfig)
    assert str(resolved.data_clients["BINANCE_PERP"].venue) == "BINANCE_PERP"
    assert str(resolved.data_clients["BINANCE_SPOT"].venue) == "BINANCE_SPOT"


def test_resolve_strategy_venues_sets_binance_testnet_defaults_in_paper() -> None:
    resolved = resolve_strategy_venues(
        config={
            "venues": {
                "execution_venue": "BINANCE_PERP",
                "reference_venue": "BINANCE_SPOT",
            },
            "node": {
                "venues": {
                    "BINANCE_PERP": {
                        "adapter": "binance",
                        "instrument_id": "PLUMEUSDT-PERP.BINANCE_PERP",
                        "account_type": "USDT_FUTURES",
                        "execution": True,
                    },
                    "BINANCE_SPOT": {
                        "adapter": "binance",
                        "instrument_id": "PLUMEUSDT.BINANCE_SPOT",
                        "account_type": "SPOT",
                    },
                },
            },
        },
        mode="paper",
        enable_execution=True,
    )

    assert isinstance(resolved.data_clients["BINANCE_PERP"], BinanceDataClientConfig)
    assert isinstance(resolved.exec_clients["BINANCE_PERP"], BinanceExecClientConfig)
    assert resolved.data_clients["BINANCE_PERP"].environment == BinanceEnvironment.TESTNET
    assert resolved.exec_clients["BINANCE_PERP"].environment == BinanceEnvironment.TESTNET
    assert resolved.data_clients["BINANCE_SPOT"].environment == BinanceEnvironment.TESTNET


def test_resolve_strategy_venues_sets_okx_demo_defaults_in_testnet() -> None:
    resolved = resolve_strategy_venues(
        config={
            "venues": {
                "execution_venue": "OKX",
                "reference_venue": "OKX",
            },
            "node": {
                "venues": {
                    "OKX": {
                        "adapter": "okx",
                        "instrument_id": "PLUME-USDT-SWAP.OKX",
                        "instrument_type": "SWAP",
                        "contract_type": "LINEAR",
                        "execution": True,
                    },
                },
            },
        },
        mode="testnet",
        enable_execution=True,
    )

    assert set(resolved.data_clients) == {OKX}
    assert set(resolved.exec_clients) == {OKX}
    assert isinstance(resolved.data_clients[OKX], OKXDataClientConfig)
    assert isinstance(resolved.exec_clients[OKX], OKXExecClientConfig)
    assert resolved.data_clients[OKX].is_demo is True
    assert resolved.exec_clients[OKX].is_demo is True


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
    with pytest.raises(ValueError, match=r"node\.venues\.BYBIT\.recv_window_ms"):
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


def test_resolve_strategy_venues_supports_hyperliquid_dex_and_account_address() -> None:
    resolved = resolve_strategy_venues(
        config={
            "venues": {
                "execution_venue": "HYPERLIQUID",
                "reference_venue": "HYPERLIQUID",
            },
            "node": {
                "venues": {
                    "HYPERLIQUID": {
                        "instrument_id": "AAPL-USD-PERP.HYPERLIQUID",
                        "execution": True,
                        "private_key": "0xdeadbeef",
                        "account_address": "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "dex": "xyz",
                    },
                },
            },
        },
        mode="paper",
        enable_execution=True,
    )

    assert resolved.execution_venue == "HYPERLIQUID"
    assert resolved.reference_venue == "HYPERLIQUID"
    assert str(resolved.execution_instrument_id) == "AAPL-USD-PERP.HYPERLIQUID"
    assert str(resolved.reference_instrument_id) == "AAPL-USD-PERP.HYPERLIQUID"
    assert set(resolved.data_clients) == {HYPERLIQUID}
    assert set(resolved.exec_clients) == {HYPERLIQUID}
    assert isinstance(resolved.data_clients[HYPERLIQUID], HyperliquidDataClientConfig)
    assert isinstance(resolved.exec_clients[HYPERLIQUID], HyperliquidExecClientConfig)
    assert resolved.data_clients[HYPERLIQUID].dex == "xyz"
    assert resolved.exec_clients[HYPERLIQUID].dex == "xyz"
    assert (
        resolved.exec_clients[HYPERLIQUID].account_address
        == "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    )

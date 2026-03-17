from __future__ import annotations

import pytest

from flux.runners.live.venues import resolve_strategy_venues
from nautilus_trader.adapters.binance import BINANCE
from nautilus_trader.adapters.binance import BinanceDataClientConfig
from nautilus_trader.adapters.binance import BinanceExecClientConfig
from nautilus_trader.adapters.binance.common.enums import BinanceEnvironment
from nautilus_trader.adapters.bitget.constants import BITGET
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


def test_resolve_strategy_venues_supports_bitget_perp_execution_and_binance_spot_reference() -> (
    None
):
    resolved = resolve_strategy_venues(
        config={
            "venues": {
                "execution_venue": "BITGET",
                "reference_venue": "BINANCE_SPOT",
            },
            "node": {
                "venues": {
                    "BITGET": {
                        "adapter": "bitget",
                        "instrument_id": "PLUMEUSDT-PERP.BITGET",
                        "product_type": "USDT_FUTURES",
                        "execution": True,
                        "api_passphrase": "passphrase",
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

    assert resolved.execution_venue == "BITGET"
    assert resolved.reference_venue == "BINANCE_SPOT"
    assert str(resolved.execution_instrument_id) == "PLUMEUSDT-PERP.BITGET"
    assert str(resolved.reference_instrument_id) == "PLUMEUSDT.BINANCE_SPOT"
    assert set(resolved.data_clients) == {BITGET, "BINANCE_SPOT"}
    assert set(resolved.exec_clients) == {BITGET}
    assert resolved.data_clients[BITGET].api_passphrase == "passphrase"
    assert resolved.exec_clients[BITGET].api_passphrase == "passphrase"


def test_resolve_strategy_venues_supports_ibkr_reference_data_client() -> None:
    interactive_brokers_config = pytest.importorskip(
        "nautilus_trader.adapters.interactive_brokers.config",
    )
    InteractiveBrokersDataClientConfig = (
        interactive_brokers_config.InteractiveBrokersDataClientConfig
    )
    SymbologyMethod = interactive_brokers_config.SymbologyMethod

    resolved = resolve_strategy_venues(
        config={
            "venues": {
                "execution_venue": "HYPERLIQUID",
                "reference_venue": "IBKR",
            },
            "node": {
                "venues": {
                    "HYPERLIQUID": {
                        "adapter": "hyperliquid",
                        "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                        "execution": True,
                    },
                    "IBKR": {
                        "adapter": "interactive_brokers",
                        "instrument_id": "AAPL.NASDAQ",
                        "ibg_host": "127.0.0.1",
                        "ibg_port": 4002,
                        "ibg_client_id": 7,
                        "execution": False,
                    },
                },
            },
        },
        mode="paper",
        enable_execution=True,
    )

    assert resolved.execution_venue == "HYPERLIQUID"
    assert resolved.reference_venue == "IBKR"
    assert str(resolved.execution_instrument_id) == "xyz:AAPL-USD-PERP.HYPERLIQUID"
    assert str(resolved.reference_instrument_id) == "AAPL.NASDAQ"
    assert set(resolved.data_clients) == {HYPERLIQUID, "IBKR"}
    assert set(resolved.exec_clients) == {HYPERLIQUID}
    assert isinstance(resolved.data_clients["IBKR"], InteractiveBrokersDataClientConfig)
    assert resolved.data_clients["IBKR"].ibg_host == "127.0.0.1"
    assert resolved.data_clients["IBKR"].ibg_port == 4002
    assert resolved.data_clients["IBKR"].ibg_client_id == 7
    assert resolved.data_clients["IBKR"].routing.venues == frozenset({"IBKR", "NASDAQ"})
    assert (
        resolved.data_clients["IBKR"].instrument_provider.symbology_method
        == SymbologyMethod.IB_SIMPLIFIED
    )


def test_resolve_strategy_venues_supports_ibkr_reference_exec_client() -> None:
    interactive_brokers_config = pytest.importorskip(
        "nautilus_trader.adapters.interactive_brokers.config",
    )
    InteractiveBrokersDataClientConfig = (
        interactive_brokers_config.InteractiveBrokersDataClientConfig
    )
    InteractiveBrokersExecClientConfig = (
        interactive_brokers_config.InteractiveBrokersExecClientConfig
    )

    resolved = resolve_strategy_venues(
        config={
            "venues": {
                "execution_venue": "HYPERLIQUID",
                "reference_venue": "IBKR",
            },
            "node": {
                "venues": {
                    "HYPERLIQUID": {
                        "adapter": "hyperliquid",
                        "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                        "execution": True,
                    },
                    "IBKR": {
                        "adapter": "interactive_brokers",
                        "instrument_id": "AAPL.NASDAQ",
                        "ibg_host": "127.0.0.1",
                        "ibg_port": 4001,
                        "ibg_client_id": 23,
                        "account_id": "U1234567",
                        "execution": True,
                    },
                },
            },
        },
        mode="live",
        enable_execution=True,
    )

    assert set(resolved.data_clients) == {HYPERLIQUID, "IBKR"}
    assert set(resolved.exec_clients) == {HYPERLIQUID, "IBKR"}
    assert isinstance(resolved.data_clients["IBKR"], InteractiveBrokersDataClientConfig)
    assert isinstance(resolved.exec_clients["IBKR"], InteractiveBrokersExecClientConfig)
    assert resolved.exec_clients[HYPERLIQUID].routing.default is False
    assert resolved.exec_clients["IBKR"].routing.default is False
    assert resolved.exec_clients["IBKR"].ibg_port == 4001
    assert resolved.exec_clients["IBKR"].ibg_client_id == 23
    assert resolved.exec_clients["IBKR"].account_id == "U1234567"


def test_resolve_strategy_venues_coerces_ibkr_dockerized_gateway_config() -> None:
    interactive_brokers_config = pytest.importorskip(
        "nautilus_trader.adapters.interactive_brokers.config",
    )
    DockerizedIBGatewayConfig = interactive_brokers_config.DockerizedIBGatewayConfig

    resolved = resolve_strategy_venues(
        config={
            "venues": {
                "execution_venue": "HYPERLIQUID",
                "reference_venue": "IBKR",
            },
            "node": {
                "venues": {
                    "HYPERLIQUID": {
                        "adapter": "hyperliquid",
                        "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                        "execution": True,
                    },
                    "IBKR": {
                        "adapter": "interactive_brokers",
                        "instrument_id": "AAPL.NASDAQ",
                        "dockerized_gateway": {
                            "trading_mode": "live",
                            "read_only_api": True,
                            "vnc_port": 5900,
                        },
                        "execution": False,
                    },
                },
            },
        },
        mode="paper",
        enable_execution=True,
    )

    dockerized_gateway = resolved.data_clients["IBKR"].dockerized_gateway

    assert isinstance(dockerized_gateway, DockerizedIBGatewayConfig)
    assert dockerized_gateway.trading_mode == "live"
    assert dockerized_gateway.read_only_api is True
    assert dockerized_gateway.vnc_port == 5900


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


def test_resolve_strategy_venues_supports_hyperliquid_dex_account_and_vault_addresses() -> None:
    resolved = resolve_strategy_venues(
        config={
            "venues": {
                "execution_venue": "HYPERLIQUID",
                "reference_venue": "HYPERLIQUID",
            },
            "node": {
                "venues": {
                    "HYPERLIQUID": {
                        "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                        "execution": True,
                        "private_key": "0xdeadbeef",
                        "account_address": "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "vault_address": "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
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
    assert str(resolved.execution_instrument_id) == "xyz:AAPL-USD-PERP.HYPERLIQUID"
    assert str(resolved.reference_instrument_id) == "xyz:AAPL-USD-PERP.HYPERLIQUID"
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
    assert (
        resolved.exec_clients[HYPERLIQUID].vault_address
        == "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    )


def test_resolve_strategy_venues_coerces_ibkr_gateway_restart_schedule() -> None:
    DockerizedIBGatewayConfig = pytest.importorskip(
        "nautilus_trader.adapters.interactive_brokers.config",
    ).DockerizedIBGatewayConfig

    resolved = resolve_strategy_venues(
        config={
            "venues": {
                "execution_venue": "HYPERLIQUID",
                "reference_venue": "IBKR",
            },
            "node": {
                "venues": {
                    "HYPERLIQUID": {
                        "adapter": "hyperliquid",
                        "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                        "execution": True,
                    },
                    "IBKR": {
                        "adapter": "interactive_brokers",
                        "instrument_id": "AAPL.NASDAQ",
                        "dockerized_gateway": {
                            "trading_mode": "live",
                            "read_only_api": True,
                            "auto_restart_time": "11:45 PM",
                            "time_zone": "America/New_York",
                            "relogin_after_twofa_timeout": True,
                        },
                    },
                },
            },
        },
        mode="paper",
        enable_execution=True,
    )

    dockerized_gateway = resolved.data_clients["IBKR"].dockerized_gateway
    assert isinstance(dockerized_gateway, DockerizedIBGatewayConfig)
    assert dockerized_gateway.trading_mode == "live"
    assert dockerized_gateway.auto_restart_time == "11:45 PM"
    assert dockerized_gateway.time_zone == "America/New_York"
    assert dockerized_gateway.relogin_after_twofa_timeout is True

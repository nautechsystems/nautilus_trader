from __future__ import annotations

import pytest

from nautilus_trader.flux.common.account_scopes import decode_account_scopes
from nautilus_trader.flux.common.controller_scopes import decode_controller_scopes
from nautilus_trader.flux.common.controller_scopes import validate_controller_scope_contracts
from nautilus_trader.flux.common.strategy_contracts import decode_strategy_contracts


def _account_scope_rows() -> list[dict[str, object]]:
    return [
        {
            "scope_id": "hyperliquid.xyz.main",
            "provider": "hyperliquid",
            "venue": "HYPERLIQUID",
            "account_address_env": "TRADE_XYZ_ACCOUNT_ADDRESS",
            "vault_address_env": "TRADE_XYZ_VAULT_ADDRESS",
        },
        {
            "scope_id": "binance.futures.main",
            "provider": "binance",
            "venue": "BINANCE_PERP",
            "api_key_env": "BINANCE_API_KEY",
            "api_secret_env": "BINANCE_API_SECRET",
            "account_type": "USDT_FUTURES",
            "private_api_family": "PORTFOLIO_MARGIN",
        },
        {
            "scope_id": "ibkr.reference.main",
            "provider": "ibkr",
            "venue": "IBKR",
            "account_id": "U10015777",
            "ibg_client_id": 107,
        },
        {
            "scope_id": "ibkr.hedge.main",
            "provider": "ibkr",
            "venue": "IBKR",
            "account_id": "U10015777",
            "ibg_client_id": 208,
            "controller_scope_id": "equities.ibkr.hedge.main",
        },
    ]


def _strategy_contract_rows(
    *,
    controller_scope_id: str | None = "equities.ibkr.hedge.main",
    execution_account_scope_id: str = "hyperliquid.xyz.main",
    hedge_account_scope_id: str | None = "ibkr.hedge.main",
) -> list[dict[str, str]]:
    row: dict[str, str] = {
        "strategy_id": "aapl_tradexyz_maker",
        "portfolio_asset_id": "AAPL",
        "maker_instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
        "reference_instrument_id": "AAPL.NASDAQ",
        "execution_account_scope_id": execution_account_scope_id,
        "reference_account_scope_id": "ibkr.reference.main",
    }
    if hedge_account_scope_id is not None:
        row["hedge_account_scope_id"] = hedge_account_scope_id
    if controller_scope_id is not None:
        row["controller_scope_id"] = controller_scope_id
    return [row]


def _controller_scope_rows(
    *,
    controller_scope_id: str = "equities.ibkr.hedge.main",
    writer_account_scope_id: str = "ibkr.hedge.main",
    account_scope_ids: tuple[str, ...] = ("ibkr.hedge.main",),
    canary: bool = True,
) -> list[dict[str, object]]:
    return [
        {
            "controller_scope_id": controller_scope_id,
            "profile_id": "equities",
            "writer_account_scope_id": writer_account_scope_id,
            "account_scope_ids": list(account_scope_ids),
            "canary": canary,
        },
    ]


def test_controller_scope_contract_requires_manual_controller_scope_id_enumeration() -> None:
    with pytest.raises(ValueError, match="controller_scope_id"):
        decode_controller_scopes(
            [
                {
                    "profile_id": "equities",
                    "writer_account_scope_id": "ibkr.hedge.main",
                    "account_scope_ids": ["ibkr.hedge.main"],
                    "canary": True,
                },
            ],
        )


def test_controller_scope_contract_allows_shared_ibkr_logical_scopes() -> None:
    account_scope_rows = _account_scope_rows()
    account_scope_rows[2]["controller_scope_id"] = "equities.ibkr.shared"
    account_scope_rows[3]["controller_scope_id"] = "equities.ibkr.shared"

    validate_controller_scope_contracts(
        account_scopes=decode_account_scopes(account_scope_rows),
        strategy_contracts=decode_strategy_contracts(
            _strategy_contract_rows(controller_scope_id="equities.ibkr.shared"),
        ),
        controller_scopes=decode_controller_scopes(
            _controller_scope_rows(
                controller_scope_id="equities.ibkr.shared",
                account_scope_ids=("ibkr.reference.main", "ibkr.hedge.main"),
            ),
        ),
    )


def test_controller_scope_contract_rejects_mixed_writer_domains_on_one_controller() -> None:
    account_scope_rows = _account_scope_rows()
    account_scope_rows[1]["controller_scope_id"] = "equities.writer.shared"
    account_scope_rows[3]["controller_scope_id"] = "equities.writer.shared"

    with pytest.raises(ValueError, match="writer domain"):
        validate_controller_scope_contracts(
            account_scopes=decode_account_scopes(account_scope_rows),
            strategy_contracts=decode_strategy_contracts(
                _strategy_contract_rows(controller_scope_id="equities.writer.shared"),
            ),
            controller_scopes=decode_controller_scopes(
                _controller_scope_rows(
                    controller_scope_id="equities.writer.shared",
                    account_scope_ids=("ibkr.hedge.main", "binance.futures.main"),
                ),
            ),
        )


def test_controller_scope_contract_rejects_strategy_missing_managed_writer_mapping() -> None:
    with pytest.raises(ValueError, match="missing controller_scope_id"):
        validate_controller_scope_contracts(
            account_scopes=decode_account_scopes(_account_scope_rows()),
            strategy_contracts=decode_strategy_contracts(
                _strategy_contract_rows(controller_scope_id=None),
            ),
            controller_scopes=decode_controller_scopes(_controller_scope_rows()),
        )


def test_controller_scope_contract_rejects_strategy_conflicting_writer_domain() -> None:
    account_scope_rows = _account_scope_rows()
    account_scope_rows[0]["controller_scope_id"] = "equities.hyperliquid.xyz.main"

    with pytest.raises(ValueError, match="writer domain"):
        validate_controller_scope_contracts(
            account_scopes=decode_account_scopes(account_scope_rows),
            strategy_contracts=decode_strategy_contracts(
                _strategy_contract_rows(controller_scope_id="equities.hyperliquid.xyz.main"),
            ),
            controller_scopes=decode_controller_scopes(
                [
                    *_controller_scope_rows(),
                    *_controller_scope_rows(
                        controller_scope_id="equities.hyperliquid.xyz.main",
                        writer_account_scope_id="hyperliquid.xyz.main",
                        account_scope_ids=("hyperliquid.xyz.main",),
                        canary=False,
                    ),
                ],
            ),
        )

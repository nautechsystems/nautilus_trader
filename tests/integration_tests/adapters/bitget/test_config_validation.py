import pytest

from nautilus_trader.adapters.bitget.config import BitgetExecClientConfig
from nautilus_trader.adapters.bitget.execution import BitgetExecutionClient
from nautilus_trader.model.enums import AccountType


def test_exec_client_config_accepts_uta_fields() -> None:
    config = BitgetExecClientConfig(
        account_mode="UTA",
        allow_cash_borrowing=True,
        margin_mode="cross",
        position_mode="one_way",
    )

    assert config.account_mode == "UTA"
    assert config.allow_cash_borrowing is True
    assert config.margin_mode == "cross"
    assert config.position_mode == "one_way"


@pytest.mark.parametrize(
    ("kwargs", "match"),
    [
        (
            {
                "account_mode": "classic",
                "allow_cash_borrowing": True,
                "margin_mode": "cross",
                "position_mode": "one_way",
            },
            "unsupported account_mode",
        ),
        (
            {
                "account_mode": "UTA",
                "allow_cash_borrowing": True,
                "margin_mode": "isolated",
                "position_mode": "one_way",
            },
            "unsupported margin_mode",
        ),
        (
            {
                "account_mode": "UTA",
                "allow_cash_borrowing": True,
                "margin_mode": "cross",
                "position_mode": "hedge",
            },
            "unsupported position_mode",
        ),
        (
            {
                "allow_cash_borrowing": True,
            },
            "allow_cash_borrowing requires account_mode='UTA'",
        ),
    ],
)
def test_exec_client_config_rejects_unsupported_uta_fields(
    kwargs: dict[str, object],
    match: str,
) -> None:
    with pytest.raises(ValueError, match=match):
        BitgetExecClientConfig(**kwargs)


def test_derive_account_type_uses_margin_for_uta_spot_borrowing() -> None:
    account_type = BitgetExecutionClient._derive_account_type(  # type: ignore[attr-defined]
        BitgetExecClientConfig(
            account_mode="UTA",
            allow_cash_borrowing=True,
            product_types=("SPOT",),
        ),
    )

    assert account_type == AccountType.MARGIN

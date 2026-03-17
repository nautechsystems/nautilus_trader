from nautilus_trader.adapters.bitget.config import BitgetExecClientConfig


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

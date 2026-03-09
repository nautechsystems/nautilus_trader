from nautilus_trader.core import nautilus_pyo3


def test_normalize_symbol_str() -> None:
    # Arrange, Act
    result = nautilus_pyo3.tardis_normalize_symbol_str(
        symbol="BTCUSDT",
        exchange="binance-futures",
        instrument_type="perpetual",
        is_inverse=False,
    )

    # Assert
    assert result == "BTCUSDT-PERP"

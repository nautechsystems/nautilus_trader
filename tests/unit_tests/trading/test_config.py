import msgspec

from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import StrategyFactory
from nautilus_trader.examples.strategies.ema_cross import EMACross


class TestStrategyFactory:
    def test_create_from_path(self):
        # Arrange
        config = {
            "instrument_id": "AUD/USD.SIM",
            "bar_type": "AUD/USD.SIM-15-MINUTE-BID-EXTERNAL",
            "trade_size": 1_000_000,
            "fast_ema_period": 10,
            "slow_ema_period": 20,
        }
        importable = ImportableStrategyConfig(
            strategy_path="nautilus_trader.examples.strategies.ema_cross:EMACross",
            config_path="nautilus_trader.examples.strategies.ema_cross:EMACrossConfig",
            config=config,
        )

        # Act
        strategy = StrategyFactory.create(importable)

        # Assert
        assert isinstance(strategy, EMACross)
        assert (
            repr(config)
            == "{'instrument_id': 'AUD/USD.SIM', 'bar_type': 'AUD/USD.SIM-15-MINUTE-BID-EXTERNAL',"
            " 'trade_size': 1000000, 'fast_ema_period': 10, 'slow_ema_period': 20}"
        )

    def test_create_from_raw(self):
        # Arrange
        raw = msgspec.json.encode(
            {
                "strategy_path": "nautilus_trader.examples.strategies.volatility_market_maker:VolatilityMarketMaker",
                "config_path": "nautilus_trader.examples.strategies.volatility_market_maker:VolatilityMarketMakerConfig",
                "config": {
                    "instrument_id": "ETHUSDT-PERP.BINANCE",
                    "bar_type": "ETHUSDT-PERP.BINANCE-1-MINUTE-LAST-EXTERNAL",
                    "atr_period": "20",
                    "atr_multiple": "6.0",
                    "trade_size": "0.01",
                },
            },
        )

        # Act
        config = ImportableStrategyConfig.parse(raw)

        # Assert
        assert isinstance(config, ImportableStrategyConfig)
        assert config.config["instrument_id"] == "ETHUSDT-PERP.BINANCE"
        assert config.config["atr_period"] == "20"

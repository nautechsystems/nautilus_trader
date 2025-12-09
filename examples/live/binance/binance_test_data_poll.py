from nautilus_trader.adapters.binance import (
    BINANCE,
    BinanceAccountType,
    BinanceDataClientConfig,
    BinanceInstrumentProviderConfig,
    BinanceLiveDataClientFactory,
)
from nautilus_trader.config import LiveExecEngineConfig, LoggingConfig, TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import InstrumentId, TraderId
from nautilus_trader.test_kit.strategies.tester_data import DataTester, DataTesterConfig

instrument_ids = [InstrumentId.from_str("BTCUSDT-PERP.BINANCE")]
bar_types = [BarType.from_str("BTCUSDT-PERP.BINANCE-1-MINUTE-LAST-EXTERNAL")]


def main() -> None:
    node = TradingNode(
        config=TradingNodeConfig(
            trader_id=TraderId("DATA-TESTER"),
            logging=LoggingConfig(log_level="INFO", use_pyo3=True),
            exec_engine=LiveExecEngineConfig(reconciliation=False),
            data_clients={
                BINANCE: BinanceDataClientConfig(
                    account_type=BinanceAccountType.USDT_FUTURES,
                    testnet=False,  # True for testnet
                    instrument_provider=BinanceInstrumentProviderConfig(
                        load_ids=frozenset(instrument_ids),
                    ),
                ),
            },
        )
    )

    strategy = DataTester(
        config=DataTesterConfig(
            instrument_ids=instrument_ids,
            bar_types=bar_types,
            subscribe_trades=True,
            subscribe_quotes=True,
            subscribe_book_at_interval=True,
            book_interval_ms=100,  # must be >0; use subscribe_book_deltas for unthrottled
        )
    )
    node.trader.add_actor(strategy)
    node.add_data_client_factory(BINANCE, BinanceLiveDataClientFactory)
    node.build()

    try:
        node.run()
    finally:
        node.dispose()


if __name__ == "__main__":
    main()

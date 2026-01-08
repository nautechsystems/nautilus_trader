#!/usr/bin/env python3
"""
TQQQ SMA200 Strategy - Live Trading Runner
NautilusTrader + IBKR Gateway
"""

from nautilus_trader.adapters.interactive_brokers.config import (
    InteractiveBrokersDataClientConfig,
    InteractiveBrokersExecClientConfig,
    InteractiveBrokersInstrumentProviderConfig,
)
from nautilus_trader.adapters.interactive_brokers.factories import (
    InteractiveBrokersLiveDataClientFactory,
    InteractiveBrokersLiveExecClientFactory,
)
from nautilus_trader.config import LoggingConfig, TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import InstrumentId

from tqqq_sma200_strategy import TQQQSMA200Config, TQQQSMA200Strategy


# =============================================================================
# IBKR Gateway 설정
# =============================================================================
IBG_HOST = "127.0.0.1"
IBG_PORT = 4002  # Paper: 4002, Live: 4001
IBG_CLIENT_ID = 1
ACCOUNT_ID = "DUO476779"  # Paper trading account

# =============================================================================
# 종목 설정
# =============================================================================
# NautilusTrader IB Simplified Symbology
SPY_INSTRUMENT = "SPY.ARCA"
TQQQ_INSTRUMENT = "TQQQ.NASDAQ"

# Bar types (1-DAY bars for SMA calculation)
SPY_BAR_TYPE = f"{SPY_INSTRUMENT}-1-DAY-LAST-EXTERNAL"
TQQQ_BAR_TYPE = f"{TQQQ_INSTRUMENT}-1-HOUR-LAST-EXTERNAL"  # Hourly for risk checks

# =============================================================================
# 전략 설정
# =============================================================================
ALLOCATION_PCT = 0.80     # 80% 배분
STOP_LOSS_PCT = 0.15      # 15% 손절
TAKE_PROFIT_PCT = 0.50    # 50% 익절
SMA_PERIOD = 200          # 200일 SMA

# =============================================================================
# Instrument Provider 설정
# =============================================================================
instrument_provider = InteractiveBrokersInstrumentProviderConfig(
    build_futures_chain=False,
    build_options_chain=False,
    load_ids=frozenset([SPY_INSTRUMENT, TQQQ_INSTRUMENT]),
)

# =============================================================================
# Trading Node 설정
# =============================================================================
config = TradingNodeConfig(
    trader_id="TQQQ-SMA200-001",
    logging=LoggingConfig(log_level="INFO"),
    data_clients={
        "IB": InteractiveBrokersDataClientConfig(
            ibg_host=IBG_HOST,
            ibg_port=IBG_PORT,
            ibg_client_id=IBG_CLIENT_ID,
            instrument_provider=instrument_provider,
            use_regular_trading_hours=True,
        ),
    },
    exec_clients={
        "IB": InteractiveBrokersExecClientConfig(
            ibg_host=IBG_HOST,
            ibg_port=IBG_PORT,
            ibg_client_id=IBG_CLIENT_ID,
            account_id=ACCOUNT_ID,
            instrument_provider=instrument_provider,
        ),
    },
    timeout_connection=60.0,
    timeout_reconciliation=10.0,
    timeout_portfolio=10.0,
    timeout_disconnection=5.0,
)


def main():
    print("\n" + "=" * 60)
    print("TQQQ SMA200 Strategy - NautilusTrader")
    print("=" * 60)
    print(f"Gateway: {IBG_HOST}:{IBG_PORT}")
    print(f"SPY Bar: {SPY_BAR_TYPE}")
    print(f"TQQQ Bar: {TQQQ_BAR_TYPE}")
    print(f"Allocation: {ALLOCATION_PCT * 100:.0f}%")
    print(f"SMA Period: {SMA_PERIOD}")
    print(f"Stop Loss: {STOP_LOSS_PCT * 100:.0f}%")
    print(f"Take Profit: {TAKE_PROFIT_PCT * 100:.0f}%")
    print("=" * 60 + "\n")

    # Create TradingNode
    node = TradingNode(config=config)

    # Register client factories
    node.add_data_client_factory("IB", InteractiveBrokersLiveDataClientFactory)
    node.add_exec_client_factory("IB", InteractiveBrokersLiveExecClientFactory)

    # Configure strategy
    strategy_config = TQQQSMA200Config(
        spy_instrument_id=InstrumentId.from_str(SPY_INSTRUMENT),
        tqqq_instrument_id=InstrumentId.from_str(TQQQ_INSTRUMENT),
        spy_bar_type=BarType.from_str(SPY_BAR_TYPE),
        tqqq_bar_type=BarType.from_str(TQQQ_BAR_TYPE),
        sma_period=SMA_PERIOD,
        allocation_pct=ALLOCATION_PCT,
        stop_loss_pct=STOP_LOSS_PCT,
        take_profit_pct=TAKE_PROFIT_PCT,
    )

    # Create and add strategy
    strategy = TQQQSMA200Strategy(config=strategy_config)
    node.trader.add_strategy(strategy)

    # Build and run
    node.build()

    try:
        node.run()
    except KeyboardInterrupt:
        print("\n종료 중...")
    finally:
        node.dispose()
        print("완료!")


if __name__ == "__main__":
    main()

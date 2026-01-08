#!/usr/bin/env python3
"""
MNQ 3x + 이중SMA + GDX 라이브 트레이딩

NautilusTrader + IBKR Gateway 연동
CONTFUT (연속 선물) 사용으로 자동 롤오버

사용법:
    # Paper Trading (모의투자)
    python run_live.py

    # Live Trading (실거래) - 주의!
    python run_live.py --live

    # 상태 확인만
    python run_live.py --status
"""

import argparse

from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.config import (
    InteractiveBrokersDataClientConfig,
    InteractiveBrokersExecClientConfig,
    InteractiveBrokersInstrumentProviderConfig,
)
from nautilus_trader.adapters.interactive_brokers.factories import (
    InteractiveBrokersLiveDataClientFactory,
    InteractiveBrokersLiveExecClientFactory,
)
from nautilus_trader.config import LiveExecEngineConfig, LoggingConfig, TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import InstrumentId, TraderId

from config import config
from mnq_dual_sma_strategy import MNQDualSMAConfig, MNQDualSMAStrategy
from slack_bot import SlackNotifier
from slack_chatbot import TradingChatbot


# =============================================================================
# IBKR 계약 정의
# =============================================================================

def create_ib_contracts():
    """
    IBKR 계약 생성.

    CONTFUT (연속 선물) 사용으로 자동 롤오버.
    """
    contracts = [
        # QQQ ETF - 시그널용
        IBContract(
            secType="STK",
            symbol="QQQ",
            exchange="SMART",
            primaryExchange="NASDAQ",
        ),
        # MNQ 연속 선물 - IBKR이 자동 롤오버
        IBContract(
            secType="CONTFUT",
            symbol=config.MNQ_SYMBOL,
            exchange=config.MNQ_EXCHANGE,
        ),
        # GDX ETF - 헤지용
        IBContract(
            secType="STK",
            symbol=config.HEDGE_SYMBOL,
            exchange="SMART",
            primaryExchange="ARCA",
        ),
    ]
    return contracts


def create_instrument_ids():
    """Create instrument IDs for the strategy."""
    # IB_SIMPLIFIED symbology 사용
    qqq_id = InstrumentId.from_str("QQQ.NASDAQ")
    # CONTFUT은 심볼.거래소 형태로 표현
    long_id = InstrumentId.from_str(f"{config.MNQ_SYMBOL}.{config.MNQ_EXCHANGE}")
    hedge_id = InstrumentId.from_str(f"{config.HEDGE_SYMBOL}.ARCA")

    return qqq_id, long_id, hedge_id


def create_bar_types(qqq_id, long_id, hedge_id):
    """Create bar types for daily bars."""
    qqq_bar = BarType.from_str(f"{qqq_id}-1-DAY-LAST-EXTERNAL")
    long_bar = BarType.from_str(f"{long_id}-1-DAY-LAST-EXTERNAL")
    hedge_bar = BarType.from_str(f"{hedge_id}-1-DAY-LAST-EXTERNAL")
    return qqq_bar, long_bar, hedge_bar


# =============================================================================
# Trading Node 구성
# =============================================================================

def build_trading_node(paper: bool = True) -> TradingNode:
    """
    Build and configure the TradingNode with IBKR adapter.

    Parameters
    ----------
    paper : bool
        True for paper trading, False for live trading.
    """
    port = config.IBKR_PAPER_PORT if paper else config.IBKR_LIVE_PORT
    mode_str = "PAPER" if paper else "LIVE"

    print("=" * 60)
    print(f"MNQ 3x + 이중SMA + GDX 전략")
    print(f"Mode: {mode_str} Trading")
    print(f"IBKR Gateway: {config.IBKR_HOST}:{port}")
    print("=" * 60)

    # IBContract으로 계약 정의
    ib_contracts = create_ib_contracts()

    print("\n[로드할 계약]")
    for contract in ib_contracts:
        if contract.secType == "CONTFUT":
            print(f"  {contract.symbol}.{contract.exchange} (연속 선물 - 자동 롤오버)")
        else:
            print(f"  {contract.symbol}.{contract.exchange or contract.primaryExchange}")

    # Instrument Provider Config
    instrument_provider_config = InteractiveBrokersInstrumentProviderConfig(
        load_contracts=frozenset(ib_contracts),
    )

    # Data Client Config
    data_client_config = InteractiveBrokersDataClientConfig(
        ibg_host=config.IBKR_HOST,
        ibg_port=port,
        ibg_client_id=config.IBKR_CLIENT_ID,
        instrument_provider=instrument_provider_config,
    )

    # Exec Client Config
    exec_client_config = InteractiveBrokersExecClientConfig(
        ibg_host=config.IBKR_HOST,
        ibg_port=port,
        ibg_client_id=config.IBKR_CLIENT_ID + 1,
        account_id=config.IBKR_ACCOUNT or None,
        instrument_provider=instrument_provider_config,
    )

    # Trading Node Config
    node_config = TradingNodeConfig(
        trader_id=TraderId("MNQ_DUAL_SMA-001"),
        logging=LoggingConfig(
            log_level="INFO",
            log_level_file="DEBUG",
            log_file_format="json",
        ),
        exec_engine=LiveExecEngineConfig(
            reconciliation=True,
            reconciliation_lookback_mins=1440,
        ),
        data_clients={
            "IB": data_client_config,
        },
        exec_clients={
            "IB": exec_client_config,
        },
        timeout_connection=30.0,
        timeout_reconciliation=10.0,
        timeout_portfolio=10.0,
        timeout_disconnection=5.0,
        timeout_post_stop=2.0,
    )

    # Build node
    node = TradingNode(config=node_config)

    # Register IBKR factories
    node.add_data_client_factory("IB", InteractiveBrokersLiveDataClientFactory)
    node.add_exec_client_factory("IB", InteractiveBrokersLiveExecClientFactory)

    # Build
    node.build()

    return node


def add_strategy(node: TradingNode) -> None:
    """Add the MNQ Dual SMA strategy to the node."""
    qqq_id, long_id, hedge_id = create_instrument_ids()
    qqq_bar, long_bar, hedge_bar = create_bar_types(qqq_id, long_id, hedge_id)

    strategy_config = MNQDualSMAConfig(
        strategy_id="MNQ_DUAL_SMA-001",
        qqq_instrument_id=qqq_id,
        long_instrument_id=long_id,
        hedge_instrument_id=hedge_id,
        qqq_bar_type=qqq_bar,
        long_bar_type=long_bar,
        hedge_bar_type=hedge_bar,
        sma_long_period=config.SMA_LONG_PERIOD,
        sma_short_period=config.SMA_SHORT_PERIOD,
        target_leverage=config.TARGET_LEVERAGE_DEFAULT,
        target_leverage_high=config.TARGET_LEVERAGE_HIGH,
        leverage_4x_threshold=config.LEVERAGE_4X_THRESHOLD,
        enable_dynamic_leverage=config.ENABLE_DYNAMIC_LEVERAGE,
        rebalance_band_pct=config.REBALANCE_BAND_PCT,
        rebalance_min_threshold=config.REBALANCE_MIN_THRESHOLD,
        contract_multiplier=config.MNQ_MULTIPLIER,  # MNQ: $2/point
        close_positions_on_stop=False,
    )

    strategy = MNQDualSMAStrategy(config=strategy_config)
    node.trader.add_strategy(strategy)

    print(f"\n[전략 설정]")
    print(f"  시그널: {qqq_id} 이중 SMA ({config.SMA_LONG_PERIOD}+{config.SMA_SHORT_PERIOD})")
    print(f"  롱: {long_id} (CONTFUT - 자동 롤오버)")
    print(f"  헤지: {hedge_id}")
    if config.ENABLE_DYNAMIC_LEVERAGE:
        print(f"  레버리지: {config.TARGET_LEVERAGE_DEFAULT}x → {config.TARGET_LEVERAGE_HIGH}x (자본 ${config.LEVERAGE_4X_THRESHOLD:,.0f} 이상시 자동 전환)")
    else:
        print(f"  레버리지: {config.TARGET_LEVERAGE_DEFAULT}x (고정)")
    print(f"  리밸런싱 밴드: ±{config.REBALANCE_BAND_PCT*100:.0f}%")
    print()


def run_trading_sync(paper: bool = True) -> None:
    """Run the trading node synchronously (blocking)."""
    node = build_trading_node(paper=paper)
    add_strategy(node)

    # Start Slack chatbot
    chatbot = TradingChatbot()
    chatbot_started = chatbot.start_async()

    # Slack notification
    notifier = SlackNotifier()
    mode = "PAPER" if paper else "LIVE"
    notifier.notify_startup(mode)

    print("\n시작 중... (Ctrl+C로 종료)")
    if chatbot_started:
        print("Slack 챗봇 활성화됨 - DM 또는 멘션으로 명령어 사용 가능")

    try:
        node.run()
    except KeyboardInterrupt:
        print("\n종료 중...")
    finally:
        if chatbot_started:
            chatbot.stop()
        node.stop()


def main():
    parser = argparse.ArgumentParser(
        description="MNQ 3x + 이중SMA + GDX 라이브 트레이딩"
    )
    parser.add_argument(
        "--live",
        action="store_true",
        help="실거래 모드 (주의: 실제 돈이 거래됩니다!)"
    )
    parser.add_argument(
        "--status",
        action="store_true",
        help="현재 상태 확인만"
    )
    args = parser.parse_args()

    paper = not args.live

    if not paper:
        print("\n" + "!" * 60)
        print("!!! 경고: 실거래 모드입니다 !!!")
        print("!!! 실제 돈이 거래됩니다 !!!")
        print("!" * 60)
        confirm = input("\n계속하시겠습니까? (yes를 입력): ")
        if confirm.lower() != "yes":
            print("취소되었습니다.")
            return

    if args.status:
        node = build_trading_node(paper=paper)
        print("\n상태 확인 완료. 연결 테스트 성공.")
        return

    run_trading_sync(paper=paper)


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""
NautilusTrader IBKR 연결 테스트
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


# IBKR Gateway 설정 (로컬 IBC 사용)
IBG_HOST = "127.0.0.1"
IBG_PORT = 4002  # Paper: 4002, Live: 4001
IBG_CLIENT_ID = 10  # 다른 연결과 충돌 방지
ACCOUNT_ID = "DUO476779"  # Paper trading account

# 로드할 종목
INSTRUMENTS = [
    "TQQQ.NASDAQ",
    "SPY.ARCA",
]

# Instrument Provider 설정
instrument_provider = InteractiveBrokersInstrumentProviderConfig(
    build_futures_chain=False,
    build_options_chain=False,
    load_ids=frozenset(INSTRUMENTS),
)

# Trading Node 설정
config = TradingNodeConfig(
    trader_id="TEST-001",
    logging=LoggingConfig(log_level="INFO"),
    data_clients={
        "IB": InteractiveBrokersDataClientConfig(
            ibg_host=IBG_HOST,
            ibg_port=IBG_PORT,
            ibg_client_id=IBG_CLIENT_ID,
            instrument_provider=instrument_provider,
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
    timeout_connection=30.0,
)

if __name__ == "__main__":
    # Node 생성 및 실행
    node = TradingNode(config=config)

    # Factory 등록
    node.add_data_client_factory("IB", InteractiveBrokersLiveDataClientFactory)
    node.add_exec_client_factory("IB", InteractiveBrokersLiveExecClientFactory)

    node.build()

    print("\n" + "=" * 50)
    print("NautilusTrader IBKR 연결 테스트")
    print("=" * 50)
    print(f"Host: {IBG_HOST}:{IBG_PORT}")
    print(f"Client ID: {IBG_CLIENT_ID}")
    print(f"Instruments: {INSTRUMENTS}")
    print("=" * 50 + "\n")

    try:
        node.run()
    except KeyboardInterrupt:
        print("\n종료 중...")
    finally:
        node.dispose()
        print("완료!")

"""
MNQ 3x + 이중SMA + GDX 전략 설정

최종 전략:
- MNQ 3x 레버리지 + 밴드 리밸런싱 (±15%)
- QQQ 이중 SMA (200+50) 시그널
- 200SMA 아래 GDX 전액매수

최소 권장 자본: $52,500 (약 7천만원)
"""

import os
from dataclasses import dataclass


@dataclass
class TradingConfig:
    """트레이딩 설정"""

    # ============== IBKR 연결 설정 ==============
    IBKR_HOST: str = "127.0.0.1"
    IBKR_PAPER_PORT: int = 4002   # Paper Trading (Gateway)
    IBKR_LIVE_PORT: int = 4001    # Live Trading (Gateway)
    IBKR_TWS_PAPER_PORT: int = 7497  # Paper Trading (TWS)
    IBKR_TWS_LIVE_PORT: int = 7496   # Live Trading (TWS)
    IBKR_CLIENT_ID: int = 1
    IBKR_ACCOUNT: str = os.getenv("IBKR_ACCOUNT", "DUO476779")  # Paper: DUO476779

    # ============== 전략 설정 ==============
    # 시그널: QQQ 이중 SMA (200+50)
    SIGNAL_SYMBOL: str = "QQQ"
    SMA_LONG_PERIOD: int = 200
    SMA_SHORT_PERIOD: int = 50

    # ============== 트레이딩 자산 ==============
    # 롱 자산: MNQ 4x (선물)
    # 헤지 자산: GDX (금광주 ETF)
    USE_FUTURES: bool = True       # MNQ 선물 사용
    LONG_SYMBOL: str = "TQQQ"      # ETF 모드 (백업용)
    HEDGE_SYMBOL: str = "GDX"      # 헤지 자산

    # MNQ 선물 설정
    MNQ_SYMBOL: str = "MNQ"
    MNQ_EXCHANGE: str = "CME"
    TARGET_LEVERAGE: float = 3.0   # 3배 레버리지 (보수적)

    # ============== 거래 비용 설정 (IBKR 기준) ==============
    # MNQ 선물
    MNQ_COMMISSION_PER_CONTRACT: float = 0.62    # 편도 커미션
    MNQ_EXCHANGE_FEE_PER_CONTRACT: float = 0.30  # 거래소 수수료
    MNQ_COST_PER_CONTRACT: float = 1.84          # 왕복 총비용 (0.62+0.30)*2
    MNQ_CONTRACT_VALUE: float = 42_000           # 1계약 노출 (NQ 21,000 기준)

    # ETF (GDX 등)
    ETF_COMMISSION_PER_SHARE: float = 0.005      # 주당 커미션 (IBKR Pro)
    ETF_MIN_COMMISSION: float = 1.00             # 최소 커미션

    # ============== 리밸런싱 설정 ==============
    # 밴드 리밸런싱: 레버리지가 밴드 벗어날 때만 조정
    REBALANCE_MODE: str = "band"                 # "daily", "band", "weekly", "monthly"
    REBALANCE_BAND_PCT: float = 0.15             # ±15% 밴드
    REBALANCE_MIN_THRESHOLD: float = 0.01        # 최소 1% 차이 있어야 리밸런싱

    # ============== 레버리지 설정 ==============
    # 예수금에 따른 동적 레버리지
    TARGET_LEVERAGE_DEFAULT: float = 3.0         # 기본 3x
    TARGET_LEVERAGE_HIGH: float = 4.0            # 자본 충분시 4x
    LEVERAGE_4X_THRESHOLD: float = 84_000        # 4x 전환 기준 ($84k 이상)

    # ============== 최소 자본 권장 ==============
    # MNQ 전략 자본 기준
    MIN_CAPITAL_3X: float = 52_500               # 3x 최소: $52.5k (~7천만원, 4계약)
    MIN_CAPITAL_4X: float = 84_000               # 4x 최소: $84k (~1.1억원, 6계약)

    # ============== Slack 알림 ==============
    SLACK_WEBHOOK_URL: str = os.getenv(
        "SLACK_WEBHOOK_URL",
        "https://hooks.slack.com/services/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"
    )
    SLACK_BOT_TOKEN: str = os.getenv(
        "SLACK_BOT_TOKEN",
        "xoxb-10240578360913-10212219290167-g8aQUdmdgBkyfwz2qAwfvqqU"
    )
    # Socket Mode용 App Token (https://api.slack.com/apps > Settings > Socket Mode)
    SLACK_APP_TOKEN: str = os.getenv(
        "SLACK_APP_TOKEN",
        "xapp-1-A0A6PK8SNBU-10260008484356-a107e622180ddcd9f6dafcf42e4fbfbe8f396cd7616b4e8abe70c5ad54d544e0"
    )

    # ============== 운영 모드 ==============
    PAPER_TRADING: bool = True     # True: 모의투자, False: 실거래

    @property
    def ibkr_port(self) -> int:
        """현재 모드에 맞는 IBKR 포트 반환"""
        return self.IBKR_PAPER_PORT if self.PAPER_TRADING else self.IBKR_LIVE_PORT

    @property
    def long_symbol(self) -> str:
        """롱 자산 심볼 반환"""
        return self.MNQ_SYMBOL if self.USE_FUTURES else self.LONG_SYMBOL


# 글로벌 설정 인스턴스
config = TradingConfig()

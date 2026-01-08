"""
MNQ 3x + 이중SMA + GDX 전략 for NautilusTrader

검증된 NautilusTrader EMA Cross 패턴 기반으로 구현.
참조: nautilus_trader/examples/strategies/ema_cross.py

전략 로직:
- QQQ > SMA200 AND QQQ > SMA50 → LONG (MNQ 연속선물)
- QQQ < SMA200 AND QQQ < SMA50 → HEDGE (GDX)
- 그 외 → 이전 포지션 유지 (히스테리시스)

리밸런싱 방식 (의도적 설계):
- 밴드 리밸런싱: 레버리지가 밴드(target ± 15%) 벗어날 때만 조정
- 일일/주간/월간 강제 리밸런싱 없음 (거래비용 ~97% 절감)
- 기본: 3x ± 15% → 2.55x~3.45x 범위 내 유지

자동 롤오버:
- CONTFUT (연속 선물) 사용으로 IBKR이 자동 처리
"""

from decimal import Decimal
from pathlib import Path

import pandas as pd

from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import PositiveFloat, PositiveInt, StrategyConfig
from nautilus_trader.core.message import Event
from nautilus_trader.indicators import SimpleMovingAverage
from nautilus_trader.model.data import Bar, BarType, QuoteTick
from nautilus_trader.model.enums import OrderSide, TimeInForce
from nautilus_trader.model.events import OrderFilled, PositionChanged, PositionClosed, PositionOpened
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.trading.strategy import Strategy

from config import config as trading_config
from slack_bot import SlackNotifier


class MNQDualSMAConfig(StrategyConfig, frozen=True):
    """
    Configuration for MNQ Dual SMA Strategy.

    Parameters
    ----------
    qqq_instrument_id : InstrumentId
        QQQ instrument ID for signal calculation.
    long_instrument_id : InstrumentId
        Long instrument ID (MNQ CONTFUT).
    hedge_instrument_id : InstrumentId
        Hedge instrument ID (GDX).
    qqq_bar_type : BarType
        The bar type for QQQ (daily bars).
    long_bar_type : BarType
        The bar type for long instrument.
    hedge_bar_type : BarType
        The bar type for hedge instrument.
    sma_long_period : int, default 200
        Long SMA period.
    sma_short_period : int, default 50
        Short SMA period.
    target_leverage : float, default 3.0
        Base target leverage for MNQ.
    target_leverage_high : float, default 4.0
        High target leverage when capital is sufficient.
    leverage_4x_threshold : float, default 84000
        Capital threshold for 4x leverage ($84k).
    rebalance_band_pct : float, default 0.15
        Rebalancing band percentage (±15%).
    close_positions_on_stop : bool, default True
        Close all positions on strategy stop.
    """

    qqq_instrument_id: InstrumentId
    long_instrument_id: InstrumentId
    hedge_instrument_id: InstrumentId
    qqq_bar_type: BarType
    long_bar_type: BarType
    hedge_bar_type: BarType
    forex_instrument_id: InstrumentId | None = None  # USD/KRW 환율 (없으면 고정 환율 사용)
    sma_long_period: PositiveInt = 200
    sma_short_period: PositiveInt = 50
    target_leverage: PositiveFloat = 3.0
    target_leverage_high: PositiveFloat = 4.0
    leverage_4x_threshold: PositiveFloat = 84_000
    enable_dynamic_leverage: bool = False  # True면 자본 증가시 4x 자동 전환
    rebalance_band_pct: PositiveFloat = 0.15
    rebalance_min_threshold: PositiveFloat = 0.01  # 최소 1% 차이 있어야 리밸런싱
    contract_multiplier: PositiveFloat = 2.0  # MNQ: $2/point
    close_positions_on_stop: bool = True


class MNQDualSMAStrategy(Strategy):
    """
    MNQ 3x + 이중SMA + GDX 전략.

    레버리지:
    - 기본: 3x 레버리지 (고정)
    - 옵션: enable_dynamic_leverage=True 시 자본 >= $84k에서 4x 자동 전환

    검증된 NautilusTrader 패턴 기반:
    - indicators_initialized() 사용
    - portfolio.is_flat() / is_net_long() 사용
    - close_all_positions() 후 새 포지션 진입

    Parameters
    ----------
    config : MNQDualSMAConfig
        The configuration for the instance.
    """

    def __init__(self, config: MNQDualSMAConfig) -> None:
        super().__init__(config)

        # Instruments (loaded in on_start)
        self.qqq_instrument: Instrument | None = None
        self.long_instrument: Instrument | None = None
        self.hedge_instrument: Instrument | None = None
        self.forex_instrument: Instrument | None = None  # USD/KRW

        # USD/KRW 환율 (실시간 업데이트, 초기값은 config에서)
        self._usd_krw_rate: float = trading_config.USD_KRW_RATE
        self._forex_rate_initialized: bool = False  # 실시간 환율 수신 여부

        # SMA indicators for QQQ
        self.sma_long = SimpleMovingAverage(config.sma_long_period)
        self.sma_short = SimpleMovingAverage(config.sma_short_period)

        # Position state for hysteresis: 'LONG', 'HEDGE', or None
        self.current_position: str | None = None
        self.state_file = Path(__file__).parent / ".nautilus_position_state"

        # Current leverage (dynamically updated based on balance)
        self._current_target_leverage: float = config.target_leverage

        # Actual instrument IDs (may differ from config due to venue mismatch)
        # High #2 수정: 포트폴리오 작업에 실제 로딩된 ID 사용
        self._actual_long_id: InstrumentId | None = None
        self._actual_hedge_id: InstrumentId | None = None

        # Actual bar types (built from actual instrument IDs)
        # MEDIUM 수정: venue mismatch 시 바 데이터 누락 방지
        self._actual_qqq_bar_type: BarType | None = None
        self._actual_long_bar_type: BarType | None = None
        self._actual_hedge_bar_type: BarType | None = None

        # Pending position switch tracking
        # High #1 수정: close_all_positions() 비동기 완료 대기
        self._pending_switch_target: str | None = None
        self._closing_instrument_id: InstrumentId | None = None

        # Position sync state
        # 리컨실리에이션 완료까지 무제한 재시도 (포지션 발견 시 완료)
        self._position_synced: bool = False
        self._sync_retry_count: int = 0
        self._sync_warn_threshold: int = 10  # 10회 이상 시 경고 로그
        self._had_state_file: bool = self.state_file.exists()  # 시작 시 상태파일 존재 여부
        self._strategy_start_time: pd.Timestamp | None = None  # 전략 시작 시간
        self._received_position_event: bool = False  # 포지션 이벤트 수신 여부 (리컨실리에이션 활성화 증거)
        self._dual_position_detected: bool = False  # LONG+HEDGE 동시 보유 감지 (Slack 중복 알림 방지)

        # 리컨실리에이션 대기 설정
        # 주의: 계좌가 완전 FLAT인 경우 포지션 이벤트가 오지 않으므로 타임아웃 폴백 사용
        # 이 값은 IBKR Gateway 연결 및 리컨실리에이션 완료에 충분한 시간이어야 함
        self._min_reconciliation_wait_seconds: int = 300  # FLAT 확정 최소 대기 시간 (5분)

        # Pending switch timeout tracking
        self._pending_switch_start_time: pd.Timestamp | None = None
        self._pending_switch_timeout_seconds: int = 120  # 청산 대기 타임아웃 (2분)

        # 주기적 포지션 재검증 (외부 수동청산/강제청산 감지)
        self._last_position_validation: pd.Timestamp | None = None
        self._position_validation_interval_seconds: int = 60  # 60초마다 재검증

        # Slack notifier
        self.slack = SlackNotifier()

        # Load last position state
        self._load_position_state()

    def _load_position_state(self) -> None:
        """Load last position state from file (for hysteresis)."""
        try:
            if self.state_file.exists():
                content = self.state_file.read_text().strip()
                if content in ("LONG", "HEDGE"):
                    self.current_position = content
                else:
                    # 상태파일 내용이 유효하지 않음 - 파일 삭제하고 FLAT 처리
                    self.current_position = None
                    self._had_state_file = False  # 리컨실리에이션 대기 로직 활성화
                    self.state_file.unlink()
        except Exception:
            self.current_position = None
            self._had_state_file = False  # 파일 읽기 실패 시에도 리셋

    def _save_position_state(self, position: str) -> None:
        """Save current position state to file."""
        try:
            self.state_file.write_text(position)
            self.current_position = position
        except Exception:
            pass

    def _get_account_balance(self) -> float:
        """
        계좌 총 잔고 조회.

        Returns
        -------
        float
            USD 총 잔고 (조회 실패 시 0.0)
        """
        # Long instrument의 venue에서 계좌 조회 시도
        if self.long_instrument is not None:
            account = self.portfolio.account(self.long_instrument.id.venue)
            if account is not None:
                return float(account.balance_total())

        # Hedge instrument의 venue에서 계좌 조회 시도
        if self.hedge_instrument is not None:
            account = self.portfolio.account(self.hedge_instrument.id.venue)
            if account is not None:
                return float(account.balance_total())

        return 0.0

    def _sync_position_with_portfolio(self) -> bool:
        """
        실제 포지션과 상태 파일 동기화.

        Returns
        -------
        bool
            동기화 완료 여부 (True=완료, False=재시도 필요)

        재시작 시 상태 파일이 실제 포지션과 다를 수 있음:
        - 파일=LONG, 실제=FLAT → 신규 진입 불가 버그
        - 파일=HEDGE, 실제=LONG → 잘못된 청산 대상

        High #2 수정: 실제 로딩된 Instrument ID 사용 (config ID 아님)
        """
        if self._actual_long_id is None or self._actual_hedge_id is None:
            self.log.warning("Cannot sync - actual instrument IDs not set yet")
            return False

        file_state = self.current_position
        actual_state = None

        # 실제 포지션 확인 (실제 로딩된 ID 사용)
        has_long = not self.portfolio.is_flat(self._actual_long_id)
        has_hedge = not self.portfolio.is_flat(self._actual_hedge_id)

        if has_long and not has_hedge:
            actual_state = "LONG"
        elif has_hedge and not has_long:
            actual_state = "HEDGE"
        elif has_long and has_hedge:
            # HIGH 수정: LONG+HEDGE 동시 보유는 비정상 상태
            # 자동 해결하지 않고 거래 차단 + 수동 대응 요청
            long_qty = abs(float(self.portfolio.net_position(self._actual_long_id)))
            hedge_qty = abs(float(self.portfolio.net_position(self._actual_hedge_id)))

            self.log.error(
                f"CRITICAL: Both LONG and HEDGE positions detected! "
                f"LONG: {long_qty:.0f}, HEDGE: {hedge_qty:.0f}. "
                f"Trading BLOCKED until manual resolution.",
                LogColor.RED,
            )

            # 최초 감지 시에만 Slack 알림 (중복 알림 방지)
            if not self._dual_position_detected:
                self._dual_position_detected = True
                self.slack.send(
                    f":rotating_light: *긴급: LONG+HEDGE 동시 보유 감지!*\n"
                    f"LONG (MNQ): {long_qty:.0f} 계약\n"
                    f"HEDGE (GDX): {hedge_qty:.0f} 주\n"
                    f"*거래가 차단되었습니다.*\n"
                    f"수동으로 한쪽 포지션을 청산한 후 봇을 재시작하세요.",
                    ":warning:",
                )

            # 동기화 완료하지 않음 → 거래 차단 상태 유지
            return False
        else:
            actual_state = None  # FLAT

        # 불일치 감지 및 보정
        if file_state != actual_state:
            self.log.warning(
                f"Position state mismatch! File: {file_state}, Actual: {actual_state}. "
                f"Using actual portfolio state.",
                LogColor.YELLOW,
            )
            self.current_position = actual_state
            if actual_state:
                self._save_position_state(actual_state)
            elif self.state_file.exists():
                self.state_file.unlink()  # FLAT이면 상태 파일 삭제
                # MEDIUM 수정: 상태파일 삭제 시 "상태파일 없음" 경로 활성화
                # 이를 통해 다음 동기화에서 이벤트/타임아웃 대기 로직을 탐
                self._had_state_file = False
            # 포트폴리오에서 포지션을 찾았으면 동기화 완료
            # FLAT으로 불일치 보정된 경우는 재시도 필요 (리컨실리에이션 미완료 가능성)
            return actual_state is not None
        else:
            # 상태 파일과 포트폴리오가 일치
            # MEDIUM 수정: 상태파일 없이 시작 + 둘 다 FLAT인 경우,
            # 포지션 이벤트 수신 OR 시간 기반 대기 필요
            if actual_state is None and not self._had_state_file:
                # 조건 1: 포지션 이벤트를 받았으면 리컨실리에이션 활성화 확인됨
                if self._received_position_event:
                    self.log.info(
                        "FLAT confirmed: received position events, reconciliation is active",
                        LogColor.GREEN,
                    )
                    # 이벤트를 받았으면 FLAT 확정 가능
                elif self._strategy_start_time is not None:
                    # 조건 2: 충분한 시간이 지났으면 FLAT 확정 (폴백)
                    elapsed = (self._clock.utc_now() - self._strategy_start_time).total_seconds()
                    if elapsed < self._min_reconciliation_wait_seconds:
                        remaining = self._min_reconciliation_wait_seconds - elapsed
                        self.log.info(
                            f"No state file, no position events yet. "
                            f"Waiting for reconciliation (elapsed: {elapsed:.0f}s, "
                            f"remaining: {remaining:.0f}s, or until position event)...",
                            LogColor.BLUE,
                        )
                        return False  # 이벤트 또는 시간 대기
                    else:
                        # 타임아웃 기반 FLAT 확정 - 주의 필요
                        # 계좌가 완전 FLAT이면 포지션 이벤트가 오지 않으므로 이 경로 사용
                        # 만약 실제 포지션이 있는데 리컨실리에이션이 늦어진 경우 중복 포지션 위험
                        self.log.warning(
                            f"FLAT confirmed after timeout ({elapsed:.0f}s) without position events. "
                            f"This is expected if account is truly FLAT. "
                            f"If positions exist but reconciliation is slow, duplicate orders may occur!",
                            LogColor.YELLOW,
                        )
                        # 잔고 정보 조회 (실시간 환율 사용)
                        balance_usd = self._get_account_balance()
                        balance_krw = balance_usd * self._usd_krw_rate
                        rate_source = "실시간" if self._forex_rate_initialized else "고정"
                        self.slack.send(
                            f":warning: 상태파일 없이 {elapsed:.0f}초 후 FLAT 확정.\n"
                            f":bank: 잔고: ${balance_usd:,.0f} (₩{balance_krw:,.0f})\n"
                            f":currency_exchange: 환율: {self._usd_krw_rate:,.0f} ({rate_source})\n"
                            f"실제 포지션이 있다면 IBKR 리컨실리에이션 확인 필요.",
                            ":hourglass:",
                        )
            self.log.info(
                f"Position state verified: {actual_state or 'FLAT'}",
                LogColor.GREEN,
            )
            return True

    def _validate_position_state(self) -> None:
        """
        주기적 포지션 재검증 (외부 수동청산/강제청산 감지).

        초기 동기화 완료 후에도 주기적으로 포트폴리오와 상태 파일 일치 확인.
        외부에서 포지션이 변경된 경우 (수동청산, 마진콜 강제청산 등)
        상태 파일을 업데이트하고 Slack 알림.
        """
        if self._actual_long_id is None or self._actual_hedge_id is None:
            return

        # 현재 상태 확인
        has_long = not self.portfolio.is_flat(self._actual_long_id)
        has_hedge = not self.portfolio.is_flat(self._actual_hedge_id)

        actual_state: str | None = None
        if has_long and not has_hedge:
            actual_state = "LONG"
        elif has_hedge and not has_long:
            actual_state = "HEDGE"
        elif has_long and has_hedge:
            # HIGH 수정: LONG+HEDGE 동시 보유는 비정상 상태 - 거래 차단
            long_qty = abs(float(self.portfolio.net_position(self._actual_long_id)))
            hedge_qty = abs(float(self.portfolio.net_position(self._actual_hedge_id)))

            self.log.error(
                f"CRITICAL: Both LONG and HEDGE positions detected during validation! "
                f"LONG: {long_qty:.0f}, HEDGE: {hedge_qty:.0f}. "
                f"Trading BLOCKED.",
                LogColor.RED,
            )

            # 거래 차단
            self._position_synced = False

            # 최초 감지 시에만 Slack 알림
            if not self._dual_position_detected:
                self._dual_position_detected = True
                self.slack.send(
                    f":rotating_light: *긴급: LONG+HEDGE 동시 보유 감지!*\n"
                    f"LONG (MNQ): {long_qty:.0f} 계약\n"
                    f"HEDGE (GDX): {hedge_qty:.0f} 주\n"
                    f"*거래가 차단되었습니다.*\n"
                    f"수동으로 한쪽 포지션을 청산한 후 봇을 재시작하세요.",
                    ":warning:",
                )
            return
        else:
            actual_state = None  # FLAT

        # 불일치 감지
        if self.current_position != actual_state:
            old_state = self.current_position
            self.log.warning(
                f"External position change detected! "
                f"Expected: {old_state or 'FLAT'}, Actual: {actual_state or 'FLAT'}",
                LogColor.RED,
            )

            # 상태 업데이트
            self.current_position = actual_state
            if actual_state:
                self._save_position_state(actual_state)
            elif self.state_file.exists():
                self.state_file.unlink()
                self._had_state_file = False

            # Slack 알림 (외부 청산/변경 감지)
            self.slack.send(
                f":rotating_light: *외부 포지션 변경 감지*\n"
                f"이전: {old_state or 'FLAT'} → 현재: {actual_state or 'FLAT'}\n"
                f"수동 청산 또는 강제 청산 가능성. 확인 필요.",
                ":warning:",
            )

    def _check_pending_switch_timeout(self) -> bool:
        """
        청산 대기 타임아웃 체크.

        Returns
        -------
        bool
            True if timeout occurred and was handled, False otherwise.
        """
        if self._pending_switch_target is None:
            return False

        if self._pending_switch_start_time is None:
            return False

        elapsed = (self._clock.utc_now() - self._pending_switch_start_time).total_seconds()
        if elapsed <= self._pending_switch_timeout_seconds:
            return False

        # 타임아웃 발생
        self.log.error(
            f"Position close TIMEOUT after {elapsed:.0f}s! "
            f"Cancelling pending switch to {self._pending_switch_target}. "
            f"Manual intervention may be required.",
            LogColor.RED,
        )
        self.slack.send(
            f":rotating_light: *청산 타임아웃!*\n"
            f"{self._closing_instrument_id} 청산이 {elapsed:.0f}초 후에도 완료되지 않음.\n"
            f"수동 확인 필요.",
            ":warning:",
        )

        # 대기 상태 리셋
        self._pending_switch_target = None
        self._closing_instrument_id = None
        self._pending_switch_start_time = None
        return True

    def _get_dynamic_leverage(self, balance: float) -> float:
        """
        예수금에 따른 동적 레버리지 계산.

        Medium #4 수정: enable_dynamic_leverage=False면 항상 target_leverage 사용.
        True일 때만 자본 충분시 4x로 자동 전환.

        4x 전환 조건 (enable_dynamic_leverage=True일 때):
        - 자본 >= threshold
        - 충분한 계약 수로 ±15% 밴드 관리 가능
        """
        # 동적 레버리지 비활성화 시 항상 기본 레버리지 사용
        if not self.config.enable_dynamic_leverage:
            return self.config.target_leverage

        threshold = self.config.leverage_4x_threshold

        if balance >= threshold:
            new_leverage = self.config.target_leverage_high
        else:
            new_leverage = self.config.target_leverage

        # 레버리지 변경 시 로깅
        if new_leverage != self._current_target_leverage:
            self.log.info(
                f"레버리지 변경: {self._current_target_leverage}x → {new_leverage}x "
                f"(잔고: ${balance:,.0f})",
                LogColor.MAGENTA,
            )
            self._current_target_leverage = new_leverage

        return new_leverage

    # =========================================================================
    # Strategy Lifecycle (검증된 패턴)
    # =========================================================================

    def _find_instrument_by_symbol(self, instrument_id: InstrumentId) -> Instrument | None:
        """
        High #2 수정: Venue ID 불일치 대응 + CONTFUT 우선.

        IBKR 환경에서 MNQ가 CONTFUT와 월물(MNQH6 등) 모두 존재할 수 있음.
        연속선물(CONTFUT)을 우선 매칭하여 자동 롤오버 보장.
        """
        import re

        # 1. 정확한 ID 매치 시도
        instrument = self.cache.instrument(instrument_id)
        if instrument is not None:
            return instrument

        # 2. 심볼로 폴백 검색 (CONTFUT 우선)
        symbol = instrument_id.symbol.value
        self.log.warning(
            f"Instrument {instrument_id} not found, searching by symbol '{symbol}'",
            LogColor.YELLOW,
        )

        # 심볼 매칭 후보 수집
        candidates = []
        for inst in self.cache.instruments():
            inst_symbol = inst.id.symbol.value
            # 정확한 심볼 매치 (MNQ == MNQ)
            if inst_symbol == symbol:
                candidates.append(inst)
            # 월물 패턴 매치 (MNQH6, MNQM6 등 - 심볼 + 월코드 + 연도)
            elif inst_symbol.startswith(symbol) and re.match(rf"^{symbol}[FGHJKMNQUVXZ]\d+$", inst_symbol):
                candidates.append(inst)

        if not candidates:
            all_ids = [str(i.id) for i in self.cache.instruments()]
            self.log.error(
                f"No instruments found for symbol '{symbol}'. Available: {all_ids}"
            )
            return None

        # CONTFUT(연속선물) 우선: 심볼이 정확히 일치하는 것 = CONTFUT
        # 월물은 MNQH6 형태로 더 긴 심볼
        contfut_candidates = [c for c in candidates if c.id.symbol.value == symbol]
        if contfut_candidates:
            result = contfut_candidates[0]
            self.log.info(
                f"Found CONTFUT: {result.id} (preferred over monthly contracts)",
                LogColor.GREEN,
            )
            return result

        # CONTFUT가 없으면 경고 후 첫 번째 월물 반환
        result = candidates[0]
        self.log.warning(
            f"CONTFUT not found for '{symbol}', using monthly contract: {result.id}. "
            f"Auto-rollover may not work!",
            LogColor.RED,
        )
        return result

    def _create_bar_type_from_instrument(
        self, instrument_id: InstrumentId, reference_bar_type: BarType
    ) -> BarType:
        """
        실제 Instrument ID로 BarType 생성 (venue mismatch 대응).

        Parameters
        ----------
        instrument_id : InstrumentId
            실제 로딩된 Instrument ID
        reference_bar_type : BarType
            config에서 지정한 BarType (스펙 추출용)

        Returns
        -------
        BarType
            실제 ID + reference 스펙으로 구성된 새 BarType

        Notes
        -----
        reference_bar_type.spec (BarSpecification)에 포함된 속성:
        - step: 바 간격 (예: 1)
        - aggregation: BarAggregation (예: DAY, HOUR, MINUTE)
        - price_type: PriceType (예: LAST, BID, ASK, MID)

        aggregation_source는 별도로 복사:
        - AggregationSource (예: EXTERNAL, INTERNAL)

        현재 NautilusTrader BarType은 이 속성들만 사용하므로 완전한 복사가 됨.
        향후 BarType에 새 속성이 추가되면 이 메서드도 업데이트 필요.
        """
        spec = reference_bar_type.spec
        return BarType(
            instrument_id=instrument_id,
            bar_spec=spec,
            aggregation_source=reference_bar_type.aggregation_source,
        )

    def on_start(self) -> None:
        """Actions to be performed on strategy start."""
        # 전략 시작 시간 기록 (리컨실리에이션 대기 시간 계산용)
        self._strategy_start_time = self._clock.utc_now()

        # Load instruments from cache (High #2: CONTFUT 우선 매칭)
        self.qqq_instrument = self._find_instrument_by_symbol(self.config.qqq_instrument_id)
        self.long_instrument = self._find_instrument_by_symbol(self.config.long_instrument_id)
        self.hedge_instrument = self._find_instrument_by_symbol(self.config.hedge_instrument_id)

        if self.qqq_instrument is None:
            self.log.error(f"Could not find instrument: {self.config.qqq_instrument_id}")
            self.stop()
            return

        if self.long_instrument is None:
            self.log.error(f"Could not find instrument: {self.config.long_instrument_id}")
            self.stop()
            return

        if self.hedge_instrument is None:
            self.log.error(f"Could not find instrument: {self.config.hedge_instrument_id}")
            self.stop()
            return

        # High #2 수정: 실제 로딩된 Instrument ID 저장
        # 이후 모든 포트폴리오 작업에 이 ID 사용 (config ID 아닌 실제 ID)
        self._actual_long_id = self.long_instrument.id
        self._actual_hedge_id = self.hedge_instrument.id

        # MEDIUM 수정: 실제 ID로 BarType 생성 (venue mismatch 대응)
        # QQQ, Long, Hedge 모두 실제 ID 기반 BarType 사용
        self._actual_qqq_bar_type = self._create_bar_type_from_instrument(
            self.qqq_instrument.id, self.config.qqq_bar_type
        )
        self._actual_long_bar_type = self._create_bar_type_from_instrument(
            self.long_instrument.id, self.config.long_bar_type
        )
        self._actual_hedge_bar_type = self._create_bar_type_from_instrument(
            self.hedge_instrument.id, self.config.hedge_bar_type
        )

        self.log.info(
            f"Actual instrument IDs - QQQ: {self.qqq_instrument.id}, "
            f"Long: {self._actual_long_id}, Hedge: {self._actual_hedge_id}",
            LogColor.CYAN,
        )

        # Venue mismatch 경고
        if self._actual_qqq_bar_type != self.config.qqq_bar_type:
            self.log.warning(
                f"QQQ bar type mismatch! Config: {self.config.qqq_bar_type}, "
                f"Actual: {self._actual_qqq_bar_type}",
                LogColor.YELLOW,
            )
        if self._actual_long_bar_type != self.config.long_bar_type:
            self.log.warning(
                f"Long bar type mismatch! Config: {self.config.long_bar_type}, "
                f"Actual: {self._actual_long_bar_type}",
                LogColor.YELLOW,
            )
        if self._actual_hedge_bar_type != self.config.hedge_bar_type:
            self.log.warning(
                f"Hedge bar type mismatch! Config: {self.config.hedge_bar_type}, "
                f"Actual: {self._actual_hedge_bar_type}",
                LogColor.YELLOW,
            )

        # Medium #1 수정: 포지션 동기화는 on_bar에서 수행 (리컨실리에이션 완료 후)
        # on_start 시점에는 포트폴리오가 비어있을 수 있음

        # Register indicators - 실제 QQQ bar type 사용
        self.register_indicator_for_bars(self._actual_qqq_bar_type, self.sma_long)
        self.register_indicator_for_bars(self._actual_qqq_bar_type, self.sma_short)

        # Request historical bars for warmup - 실제 bar type 사용
        # QQQ: SMA 계산용 (200일 필요)
        self.request_bars(
            self._actual_qqq_bar_type,
            start=self._clock.utc_now() - pd.Timedelta(days=250),
        )
        # MNQ/GDX: 가격 조회용 (주문/리밸런싱에 필요)
        self.request_bars(
            self._actual_long_bar_type,
            start=self._clock.utc_now() - pd.Timedelta(days=5),
        )
        self.request_bars(
            self._actual_hedge_bar_type,
            start=self._clock.utc_now() - pd.Timedelta(days=5),
        )

        # Subscribe to bars - 실제 ID 기반 BarType 사용
        self.subscribe_bars(self._actual_qqq_bar_type)
        self.subscribe_bars(self._actual_long_bar_type)
        self.subscribe_bars(self._actual_hedge_bar_type)

        # USD/KRW 환율 구독 (설정된 경우)
        if self.config.forex_instrument_id is not None:
            self.forex_instrument = self._find_instrument_by_symbol(
                self.config.forex_instrument_id
            )
            if self.forex_instrument is not None:
                self.subscribe_quote_ticks(self.forex_instrument.id)
                self.log.info(
                    f"USD/KRW 환율 구독: {self.forex_instrument.id}",
                    LogColor.CYAN,
                )
            else:
                self.log.warning(
                    f"USD/KRW instrument not found: {self.config.forex_instrument_id}. "
                    f"Using fixed rate: {self._usd_krw_rate:,.0f}",
                    LogColor.YELLOW,
                )

        self.log.info("=" * 60, LogColor.GREEN)
        self.log.info("MNQ 3x + 이중SMA + GDX 전략 시작", LogColor.GREEN)
        self.log.info("=" * 60, LogColor.GREEN)
        self.log.info(
            f"시그널: {self.config.qqq_instrument_id} "
            f"SMA({self.config.sma_long_period}+{self.config.sma_short_period})",
            LogColor.CYAN,
        )
        self.log.info(f"롱: {self.config.long_instrument_id} (CONTFUT)", LogColor.CYAN)
        self.log.info(f"헤지: {self.config.hedge_instrument_id}", LogColor.CYAN)
        self.log.info(
            f"레버리지: {self.config.target_leverage}x "
            f"(밴드: ±{self.config.rebalance_band_pct*100:.0f}%)",
            LogColor.CYAN,
        )
        self.log.info(f"현재 포지션: {self.current_position or 'N/A'}", LogColor.CYAN)

    def on_bar(self, bar: Bar) -> None:
        """Actions to be performed when receiving a bar."""
        # MEDIUM 수정: 청산 타임아웃 체크는 모든 바에서 실행 (일봉 대기 X)
        # MNQ/GDX 바도 수신하므로 더 자주 체크 가능
        self._check_pending_switch_timeout()

        # Only process QQQ bars for signal - 실제 QQQ bar type 사용
        qqq_bar_type = self._actual_qqq_bar_type or self.config.qqq_bar_type
        if bar.bar_type != qqq_bar_type:
            return

        # 검증된 패턴: indicators_initialized() 사용
        if not self.indicators_initialized():
            self.log.info(
                f"Waiting for indicators to warm up "
                f"[{self.cache.bar_count(qqq_bar_type)}/{self.config.sma_long_period}]",
                color=LogColor.BLUE,
            )
            return

        # 리컨실리에이션 완료까지 동기화 재시도
        # - 포지션 발견 시 완료
        # - 상태 파일과 포트폴리오 모두 FLAT이면 완료 (일치)
        # - 불일치로 FLAT 보정된 경우 계속 재시도 (리컨실리에이션 미완료 가능성)
        if not self._position_synced:
            self._sync_retry_count += 1
            sync_complete = self._sync_position_with_portfolio()

            if sync_complete:
                self._position_synced = True
                self.log.info(
                    f"Position sync completed after {self._sync_retry_count} attempts: "
                    f"{self.current_position or 'FLAT'}",
                    LogColor.GREEN,
                )
            elif self._sync_retry_count == self._sync_warn_threshold:
                # 경고 임계값 도달 시 경고 로그
                self.log.warning(
                    f"Position sync: {self._sync_retry_count} attempts, still waiting. "
                    f"State file had position but portfolio is FLAT - reconciliation may be pending.",
                    LogColor.YELLOW,
                )

        # HIGH 수정: 동기화 완료 전 거래 차단 (중복 포지션 방지)
        if not self._position_synced:
            self.log.info(
                f"Waiting for position sync (attempt {self._sync_retry_count})...",
                LogColor.BLUE,
            )
            return

        # MEDIUM 수정: 주기적 포지션 재검증 (외부 수동청산/강제청산 감지)
        # 초기 동기화 완료 후에도 주기적으로 포트폴리오와 상태 일치 확인
        now = self._clock.utc_now()
        should_validate = (
            self._last_position_validation is None
            or (now - self._last_position_validation).total_seconds()
            >= self._position_validation_interval_seconds
        )
        if should_validate:
            self._last_position_validation = now
            self._validate_position_state()

        # 포지션 전환 대기 중이면 신호 처리 스킵
        # (타임아웃 체크는 on_bar 시작 시 모든 바에서 실행됨)
        if self._pending_switch_target is not None:
            self.log.info(
                f"Waiting for position close to complete before switching to {self._pending_switch_target}",
                LogColor.BLUE,
            )
            return

        # Calculate signal
        qqq_price = float(bar.close)
        sma_long_value = self.sma_long.value
        sma_short_value = self.sma_short.value

        above_sma_long = qqq_price > sma_long_value
        above_sma_short = qqq_price > sma_short_value

        dist_long = (qqq_price - sma_long_value) / sma_long_value * 100
        dist_short = (qqq_price - sma_short_value) / sma_short_value * 100

        # Determine target position
        target_position = self._get_target_position(above_sma_long, above_sma_short)

        # Log current state
        self.log.info(
            f"QQQ ${qqq_price:.2f} | "
            f"SMA200 {dist_long:+.1f}% | SMA50 {dist_short:+.1f}% | "
            f"Signal: {target_position}",
            LogColor.CYAN,
        )

        # Execute position switch if needed
        if target_position != self.current_position:
            self._switch_position(
                target_position,
                qqq_price,
                sma_long_value,
                sma_short_value,
            )
        else:
            # Band rebalancing
            self._rebalance_if_needed()

    def _get_target_position(self, above_long: bool, above_short: bool) -> str:
        """Determine target position based on dual SMA with hysteresis."""
        if above_long and above_short:
            return "LONG"
        elif not above_long and not above_short:
            return "HEDGE"
        else:
            # Hysteresis: maintain previous position
            return self.current_position or "HEDGE"

    # =========================================================================
    # Position Switching (검증된 패턴)
    # =========================================================================

    def _switch_position(
        self,
        target: str,
        qqq_price: float,
        sma_long: float,
        sma_short: float,
    ) -> None:
        """
        Switch from current position to target position.

        High #1 수정: close_all_positions()는 비동기이므로
        청산 완료를 on_event()에서 감지한 후 새 포지션 진입.
        High #2 수정: 실제 로딩된 Instrument ID 사용.
        """
        self.log.info(
            f"Position switch: {self.current_position} → {target}",
            LogColor.MAGENTA,
        )

        # Slack notification
        self.slack.notify_signal(target, qqq_price, sma_long, sma_short)

        # 기존 포지션이 있으면 청산 후 새 포지션 진입 대기
        if self.current_position == "LONG" and self._actual_long_id:
            if not self.portfolio.is_flat(self._actual_long_id):
                self._pending_switch_target = target
                self._closing_instrument_id = self._actual_long_id
                self._pending_switch_start_time = self._clock.utc_now()  # 타임아웃 추적용
                self.log.info(
                    f"Closing LONG position, will open {target} after close completes",
                    LogColor.BLUE,
                )
                self.close_all_positions(self._actual_long_id)
                return  # on_event()에서 청산 완료 감지 후 진입

        elif self.current_position == "HEDGE" and self._actual_hedge_id:
            if not self.portfolio.is_flat(self._actual_hedge_id):
                self._pending_switch_target = target
                self._closing_instrument_id = self._actual_hedge_id
                self._pending_switch_start_time = self._clock.utc_now()  # 타임아웃 추적용
                self.log.info(
                    f"Closing HEDGE position, will open {target} after close completes",
                    LogColor.BLUE,
                )
                self.close_all_positions(self._actual_hedge_id)
                return  # on_event()에서 청산 완료 감지 후 진입

        # 기존 포지션 없거나 이미 FLAT이면 바로 새 포지션 진입
        self._open_new_position(target)

    def _open_new_position(self, target: str) -> None:
        """Open new position after confirming previous position is closed."""
        if target == "LONG":
            self._open_long_position()
        elif target == "HEDGE":
            self._open_hedge_position()

        # 상태 저장은 on_event()의 OrderFilled 이벤트에서 처리
        # 주문 실패 시 상태 불일치 방지
        self._pending_switch_target = None
        self._closing_instrument_id = None
        self._pending_switch_start_time = None  # 타임아웃 리셋

    def on_quote_tick(self, tick: QuoteTick) -> None:
        """
        Handle quote tick updates for USD/KRW exchange rate.

        Parameters
        ----------
        tick : QuoteTick
            The received quote tick.
        """
        if self.forex_instrument is None:
            return

        if tick.instrument_id != self.forex_instrument.id:
            return

        # USD/KRW mid price 계산 (bid + ask / 2)
        bid = float(tick.bid_price)
        ask = float(tick.ask_price)
        mid = (bid + ask) / 2.0

        if mid > 0:
            self._usd_krw_rate = mid
            if not self._forex_rate_initialized:
                self._forex_rate_initialized = True
                self.log.info(
                    f"USD/KRW 실시간 환율 수신: {mid:,.2f}",
                    LogColor.GREEN,
                )

    def on_event(self, event: Event) -> None:
        """
        Handle events - detect position close completion and track reconciliation.

        High #1 수정: 청산 완료 감지 후 새 포지션 진입.
        MEDIUM 수정: 포지션 이벤트 수신 추적 (리컨실리에이션 활성화 증거).
        체결 이벤트 기반 상태 저장: 주문 실패 시 상태 불일치 방지.
        """
        # 체결 이벤트 기반 포지션 상태 저장
        # 주문 제출이 아닌 실제 체결 시에만 상태 파일 업데이트
        if isinstance(event, OrderFilled):
            filled_id = event.instrument_id
            if self._actual_long_id and filled_id == self._actual_long_id:
                # LONG 포지션 체결
                if event.order_side == OrderSide.BUY:
                    self._save_position_state("LONG")
                    self.log.info(
                        f"Position state saved: LONG (filled {event.quantity} @ {event.last_px})",
                        LogColor.GREEN,
                    )
            elif self._actual_hedge_id and filled_id == self._actual_hedge_id:
                # HEDGE 포지션 체결
                if event.order_side == OrderSide.BUY:
                    self._save_position_state("HEDGE")
                    self.log.info(
                        f"Position state saved: HEDGE (filled {event.quantity} @ {event.last_px})",
                        LogColor.GREEN,
                    )

        # MEDIUM 수정: 포지션 이벤트 수신 추적
        # 리컨실리에이션이 활성화되었다는 증거로 사용
        if isinstance(event, (PositionOpened, PositionChanged, PositionClosed)):
            if not self._received_position_event:
                self._received_position_event = True
                self.log.info(
                    f"Received position event: {type(event).__name__} - "
                    f"reconciliation is active",
                    LogColor.CYAN,
                )

        # 포지션 전환 대기 중이 아니면 종료
        if self._pending_switch_target is None:
            return
        if self._closing_instrument_id is None:
            return

        # 청산 대상이 FLAT이 되었는지 확인
        if self.portfolio.is_flat(self._closing_instrument_id):
            self.log.info(
                f"Position close completed, opening {self._pending_switch_target}",
                LogColor.GREEN,
            )
            target = self._pending_switch_target
            self._open_new_position(target)

    def _open_long_position(self) -> None:
        """Open long position (MNQ CONTFUT)."""
        account = self.portfolio.account(self.long_instrument.id.venue)
        if account is None:
            self.log.error("No account found for long instrument")
            return

        balance = float(account.balance_total())
        # MEDIUM 수정: 실제 bar type 사용
        long_bar_type = self._actual_long_bar_type or self.config.long_bar_type
        last_bar = self.cache.bar(long_bar_type)
        if last_bar is None:
            self.log.error("No price data for long instrument")
            return

        price = float(last_bar.close)
        if price <= 0:
            return

        # 계약 가치 계산: 지수가격 × 승수
        # MNQ: $2/point (21,000 → $42,000)
        contract_value = price * self.config.contract_multiplier

        # 동적 레버리지 계산 (예수금에 따라 3x 또는 4x)
        target_leverage = self._get_dynamic_leverage(balance)
        target_value = balance * target_leverage
        quantity = int(target_value / contract_value)

        if quantity <= 0:
            self.log.warning("Insufficient funds for long position")
            return

        actual_leverage = (quantity * contract_value) / balance
        self.log.info(
            f"LONG: {self._actual_long_id} {quantity} contracts @ ${price:.2f} "
            f"(계약가치: ${contract_value:,.0f}, 실제 레버리지: {actual_leverage:.2f}x)",
            LogColor.GREEN,
        )

        # Slack notification (잔고 및 환율 포함)
        self.slack.notify_position_change(
            action="BUY",
            symbol=str(self._actual_long_id),
            quantity=quantity,
            price=price,
            from_position=self.current_position,
            to_position="LONG",
            balance_usd=balance,
            usd_krw_rate=self._usd_krw_rate,
            rate_is_realtime=self._forex_rate_initialized,
        )

        # High #2 수정: 실제 ID로 주문 제출
        order: MarketOrder = self.order_factory.market(
            instrument_id=self._actual_long_id,
            order_side=OrderSide.BUY,
            quantity=self.long_instrument.make_qty(Decimal(quantity)),
            time_in_force=TimeInForce.DAY,
        )
        self.submit_order(order)

    def _open_hedge_position(self) -> None:
        """Open hedge position (GDX)."""
        account = self.portfolio.account(self.hedge_instrument.id.venue)
        if account is None:
            self.log.error("No account found for hedge instrument")
            return

        balance = float(account.balance_total())
        # MEDIUM 수정: 실제 bar type 사용
        hedge_bar_type = self._actual_hedge_bar_type or self.config.hedge_bar_type
        last_bar = self.cache.bar(hedge_bar_type)
        if last_bar is None:
            self.log.error("No price data for hedge instrument")
            return

        price = float(last_bar.close)
        if price <= 0:
            return

        # GDX: 1x leverage (full position)
        quantity = int((balance * 0.95) / price)

        if quantity <= 0:
            self.log.warning("Insufficient funds for hedge position")
            return

        self.log.info(
            f"HEDGE: {self._actual_hedge_id} {quantity} shares @ ${price:.2f}",
            LogColor.YELLOW,
        )

        # Slack notification (잔고 및 환율 포함)
        self.slack.notify_position_change(
            action="BUY",
            symbol=str(self._actual_hedge_id),
            quantity=quantity,
            price=price,
            from_position=self.current_position,
            to_position="HEDGE",
            balance_usd=balance,
            usd_krw_rate=self._usd_krw_rate,
            rate_is_realtime=self._forex_rate_initialized,
        )

        # High #2 수정: 실제 ID로 주문 제출
        order: MarketOrder = self.order_factory.market(
            instrument_id=self._actual_hedge_id,
            order_side=OrderSide.BUY,
            quantity=self.hedge_instrument.make_qty(Decimal(quantity)),
            time_in_force=TimeInForce.DAY,
        )
        self.submit_order(order)

    # =========================================================================
    # Band Rebalancing
    # =========================================================================

    def _rebalance_if_needed(self) -> None:
        """
        Band rebalancing: only adjust when leverage is outside band.

        Only applies to LONG position (MNQ).
        HEDGE (GDX) is always 1x, no rebalancing needed.

        High #2 수정: 실제 로딩된 Instrument ID 사용.
        """
        if self.current_position != "LONG":
            return

        if self._actual_long_id is None:
            return

        # Check if we have a position (실제 ID 사용)
        if self.portfolio.is_flat(self._actual_long_id):
            return

        account = self.portfolio.account(self.long_instrument.id.venue)
        if account is None:
            return

        balance = float(account.balance_total())
        if balance <= 0:
            return

        # MEDIUM 수정: 실제 bar type 사용
        long_bar_type = self._actual_long_bar_type or self.config.long_bar_type
        last_bar = self.cache.bar(long_bar_type)
        if last_bar is None:
            return

        price = float(last_bar.close)
        if price <= 0:
            return

        # 계약 가치 계산: 지수가격 × 승수
        contract_value = price * self.config.contract_multiplier

        # Get current position (실제 ID 사용)
        net_position = float(self.portfolio.net_position(self._actual_long_id))
        if net_position <= 0:
            return

        # Calculate current leverage (계약 가치 × 계약 수 / 예수금)
        position_value = net_position * contract_value
        current_leverage = position_value / balance

        # 동적 레버리지 계산 (예수금에 따라 3x 또는 4x)
        target = self._get_dynamic_leverage(balance)
        band = self.config.rebalance_band_pct
        lower_bound = target * (1 - band)
        upper_bound = target * (1 + band)

        # Check if within band
        if lower_bound <= current_leverage <= upper_bound:
            return  # Within band, no action needed

        # Outside band → rebalance to target
        target_value = balance * target
        target_qty = int(target_value / contract_value)
        diff = target_qty - int(net_position)

        # Low #5: 최소 리밸런싱 임계값 적용
        # 1계약 미만이거나, 포지션 대비 변화율이 임계값 미만이면 스킵
        if abs(diff) < 1:
            return
        if net_position > 0 and abs(diff) / net_position < self.config.rebalance_min_threshold:
            return

        self.log.info(
            f"Rebalance: {current_leverage:.2f}x → {target:.1f}x "
            f"(band: {lower_bound:.2f}x~{upper_bound:.2f}x)",
            LogColor.BLUE,
        )

        # Slack notification (환율 포함)
        self.slack.notify_rebalance(
            str(self._actual_long_id),
            int(diff),
            price,
            usd_krw_rate=self._usd_krw_rate,
        )

        # High #2 수정: 실제 ID로 주문 제출
        if diff > 0:
            order = self.order_factory.market(
                instrument_id=self._actual_long_id,
                order_side=OrderSide.BUY,
                quantity=self.long_instrument.make_qty(Decimal(int(diff))),
                time_in_force=TimeInForce.DAY,
            )
            self.submit_order(order)
        else:
            order = self.order_factory.market(
                instrument_id=self._actual_long_id,
                order_side=OrderSide.SELL,
                quantity=self.long_instrument.make_qty(Decimal(abs(int(diff)))),
                time_in_force=TimeInForce.DAY,
            )
            self.submit_order(order)

    # =========================================================================
    # Strategy Lifecycle (검증된 패턴)
    # =========================================================================

    def on_stop(self) -> None:
        """Actions to be performed when the strategy is stopped."""
        # High #2 수정: 실제 ID 사용 (없으면 config ID로 폴백)
        long_id = self._actual_long_id or self.config.long_instrument_id
        hedge_id = self._actual_hedge_id or self.config.hedge_instrument_id

        self.cancel_all_orders(long_id)
        self.cancel_all_orders(hedge_id)

        if self.config.close_positions_on_stop:
            self.close_all_positions(long_id)
            self.close_all_positions(hedge_id)

        # MEDIUM 수정: 실제 bar type으로 unsubscribe
        if self._actual_qqq_bar_type:
            self.unsubscribe_bars(self._actual_qqq_bar_type)
        if self._actual_long_bar_type:
            self.unsubscribe_bars(self._actual_long_bar_type)
        if self._actual_hedge_bar_type:
            self.unsubscribe_bars(self._actual_hedge_bar_type)

        self.log.info("Strategy stopped", LogColor.RED)
        self.slack.send("Strategy stopped", ":octagonal_sign:")

    def on_reset(self) -> None:
        """Actions to be performed when the strategy is reset."""
        self.sma_long.reset()
        self.sma_short.reset()

    def on_dispose(self) -> None:
        """Cleanup resources."""
        pass

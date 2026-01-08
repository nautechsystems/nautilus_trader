#!/usr/bin/env python3
"""
Slack Chatbot for MNQ Trading System

Socket Mode를 사용한 실시간 명령어 처리

명령어:
- /ibkr status - 시장 상태
- /ibkr position - 현재 포지션
- /ibkr signal - 매매 신호
- /ibkr balance - 계좌 잔고
- /ibkr rate - 환율 정보
- /ibkr sma - SMA 상세
- /ibkr help - 도움말

또는 DM/멘션으로 한글 명령어 사용:
- 상태, 포지션, 신호, 잔고, 환율, 도움말

사용법:
    python slack_chatbot.py

설정:
    Slack App에서 Socket Mode를 활성화하고 App-Level Token을 발급받아야 합니다.
    1. https://api.slack.com/apps 에서 앱 선택
    2. Settings > Socket Mode > Enable Socket Mode
    3. App-Level Token 생성 (connections:write scope)
    4. SLACK_APP_TOKEN 환경변수 설정
"""

import os
import re
import sys
import threading
from pathlib import Path

# Try to import slack_bolt
try:
    from slack_bolt import App
    from slack_bolt.adapter.socket_mode import SocketModeHandler
    BOLT_AVAILABLE = True
except ImportError:
    BOLT_AVAILABLE = False

from slack_bot import SlackNotifier

# Config
from config import config


class TradingChatbot:
    """Slack chatbot for trading commands."""

    def __init__(self):
        self.notifier = SlackNotifier()
        self.state_file = Path(__file__).parent / ".nautilus_position_state"

        # Slack App Token (for Socket Mode)
        self.app_token = config.SLACK_APP_TOKEN or os.getenv("SLACK_APP_TOKEN", "")
        self.bot_token = config.SLACK_BOT_TOKEN

        # For async mode
        self._handler = None
        self._thread = None
        self._running = False

        if BOLT_AVAILABLE and self.app_token and self.bot_token:
            self.app = App(token=self.bot_token)
            self._register_handlers()
        else:
            self.app = None

    def _register_handlers(self):
        """Register message handlers."""

        # 멘션 또는 DM으로 메시지 받았을 때
        @self.app.event("app_mention")
        def handle_mention(event, say):
            text = event.get("text", "").lower()
            self._handle_command(text, say)

        @self.app.event("message")
        def handle_message(event, say):
            # DM만 처리 (채널에서는 멘션만)
            if event.get("channel_type") == "im":
                text = event.get("text", "").lower()
                self._handle_command(text, say)

        # /ibkr 통합 명령어
        @self.app.command("/ibkr")
        def handle_ibkr_command(ack, respond, command):
            ack()
            subcommand = command.get("text", "").strip().lower()
            response = self._handle_subcommand(subcommand)
            respond(response)

    def _handle_subcommand(self, subcommand: str) -> str:
        """Handle /ibkr subcommands."""
        if subcommand in ["status", "상태", "현황", ""]:
            return self.get_status()
        elif subcommand in ["position", "pos", "포지션", "보유"]:
            return self.get_position()
        elif subcommand in ["signal", "sig", "신호", "시그널"]:
            return self.get_signal()
        elif subcommand in ["balance", "bal", "잔고", "계좌"]:
            return self.get_balance()
        elif subcommand in ["rate", "fx", "환율"]:
            return self.get_rate()
        elif subcommand in ["sma", "이평", "이동평균"]:
            return self.get_sma_detail()
        elif subcommand in ["config", "설정", "세팅"]:
            return self.get_config()
        elif subcommand in ["risk", "위험", "리스크"]:
            return self.get_risk()
        elif subcommand in ["help", "도움", "도움말", "명령어", "?"]:
            return self.get_help()
        else:
            return f":warning: 알 수 없는 명령어: `{subcommand}`\n\n{self.get_help()}"

    def _handle_command(self, text: str, say):
        """Handle text commands (DM/mention)."""
        text = text.lower().strip()

        # Remove bot mention
        text = re.sub(r"<@[A-Z0-9]+>", "", text).strip()

        if any(cmd in text for cmd in ["status", "상태", "현황"]):
            say(self.get_status())
        elif any(cmd in text for cmd in ["position", "pos", "포지션", "보유"]):
            say(self.get_position())
        elif any(cmd in text for cmd in ["signal", "sig", "신호", "시그널"]):
            say(self.get_signal())
        elif any(cmd in text for cmd in ["balance", "bal", "잔고", "계좌"]):
            say(self.get_balance())
        elif any(cmd in text for cmd in ["rate", "fx", "환율"]):
            say(self.get_rate())
        elif any(cmd in text for cmd in ["sma", "이평", "이동평균"]):
            say(self.get_sma_detail())
        elif any(cmd in text for cmd in ["config", "설정", "세팅"]):
            say(self.get_config())
        elif any(cmd in text for cmd in ["risk", "위험", "리스크"]):
            say(self.get_risk())
        elif any(cmd in text for cmd in ["help", "도움", "도움말", "명령어", "?"]):
            say(self.get_help())
        else:
            say(self.get_help())

    def get_status(self) -> str:
        """Get comprehensive status including market, position, and balance."""
        import json

        # 1. Market status
        market = self.notifier.get_current_status()
        if "error" in market:
            market_section = f":warning: 시장 데이터 오류: {market['error']}"
        else:
            signal_emoji = ":chart_with_upwards_trend:" if market["signal"] == "LONG" else ":shield:"
            market_section = (
                f"*시장 현황*\n"
                f"• QQQ: ${market['qqq_price']:.2f}\n"
                f"• SMA200: ${market['sma200']:.2f} ({market['dist_200']:+.1f}%)\n"
                f"• SMA50: ${market['sma50']:.2f} ({market['dist_50']:+.1f}%)\n"
                f"• 신호: {signal_emoji} {market['signal']}"
            )

        # 2. Position & Balance from detailed state
        detailed_state_file = Path(__file__).parent / ".nautilus_detailed_state.json"
        position_section = ""
        balance_section = ""

        try:
            if detailed_state_file.exists():
                state = json.loads(detailed_state_file.read_text())
                position = state.get("position", "FLAT")
                symbol = state.get("symbol", "")
                quantity = state.get("quantity", 0)
                balance_usd = state.get("balance_usd", 0)
                balance_krw = state.get("balance_krw", 0)
                leverage = state.get("leverage", 0)
                target_leverage = state.get("target_leverage", 3.0)

                # Position section with futures explanation
                if position == "LONG" and quantity > 0:
                    # MNQ contract value calculation
                    mnq_price = market.get("qqq_price", 500) * 20  # NQ ≈ QQQ * 20
                    contract_value = quantity * mnq_price * 2  # $2/point
                    contract_value_krw = contract_value * config.USD_KRW_RATE

                    position_section = (
                        f"\n\n*포지션 (MNQ 선물)*\n"
                        f"• 수량: {quantity}계약\n"
                        f"• 1계약 = 나스닥100 × $2\n"
                        f"• 총 노출: ${contract_value:,.0f} (₩{contract_value_krw:,.0f})\n"
                        f"• 레버리지: {leverage:.2f}x / {target_leverage:.1f}x 목표"
                    )
                elif position == "HEDGE" and quantity > 0:
                    position_section = (
                        f"\n\n*포지션 (GDX 헤지)*\n"
                        f"• 수량: {quantity:,}주\n"
                        f"• 금광주 ETF로 헤지 중"
                    )
                else:
                    position_section = f"\n\n*포지션*\n• 없음 (FLAT)"

                # Balance section
                balance_section = (
                    f"\n\n*잔고*\n"
                    f"• ${balance_usd:,.0f} (₩{balance_krw:,.0f})"
                )
            else:
                position_section = "\n\n*포지션*\n• 데이터 없음 (봇 재시작 필요)"
        except Exception as e:
            position_section = f"\n\n*포지션*\n• 조회 오류: {e}"

        return market_section + position_section + balance_section

    def get_position(self) -> str:
        """Get current position."""
        try:
            if self.state_file.exists():
                position = self.state_file.read_text().strip()
            else:
                position = "N/A"
        except Exception:
            position = "N/A"

        if position == "LONG":
            emoji = ":chart_with_upwards_trend:"
            desc = "MNQ 4x 롱 포지션"
        elif position == "HEDGE":
            emoji = ":shield:"
            desc = "GDX 헤지 포지션"
        else:
            emoji = ":grey_question:"
            desc = "포지션 없음"

        return f"*현재 포지션*\n\n{emoji} *{position}*\n{desc}"

    def get_signal(self) -> str:
        """Get current signal."""
        status = self.notifier.get_current_status()

        if "error" in status:
            return f":warning: 오류: {status['error']}"

        # Determine signal explanation
        if status["above_200"] and status["above_50"]:
            signal = "LONG"
            emoji = ":chart_with_upwards_trend:"
            reason = "QQQ가 SMA200과 SMA50 모두 위에 있음"
            action = "MNQ 4x 롱 포지션 유지/진입"
        elif not status["above_200"] and not status["above_50"]:
            signal = "HEDGE"
            emoji = ":shield:"
            reason = "QQQ가 SMA200과 SMA50 모두 아래에 있음"
            action = "GDX 헤지 포지션 유지/진입"
        else:
            signal = "HOLD"
            emoji = ":pause_button:"
            if status["above_200"]:
                reason = "QQQ가 SMA200 위, SMA50 아래 (중간 상태)"
            else:
                reason = "QQQ가 SMA200 아래, SMA50 위 (중간 상태)"
            action = "현재 포지션 유지 (히스테리시스)"

        return (
            f"*현재 신호*\n\n"
            f"{emoji} *{signal}*\n\n"
            f"*이유:* {reason}\n"
            f"*액션:* {action}\n\n"
            f"_QQQ ${status['qqq_price']:.2f} | "
            f"SMA200 {status['dist_200']:+.1f}% | "
            f"SMA50 {status['dist_50']:+.1f}%_"
        )

    def get_balance(self) -> str:
        """Get account balance with KRW conversion."""
        # Note: 실제 잔고는 전략에서만 조회 가능. 여기서는 설정 기반 예상치 표시
        rate = config.USD_KRW_RATE
        min_capital = config.MIN_CAPITAL_3X
        min_capital_krw = min_capital * rate

        try:
            if self.state_file.exists():
                position = self.state_file.read_text().strip()
            else:
                position = "N/A"
        except Exception:
            position = "N/A"

        return (
            f"*계좌 정보*\n\n"
            f":bank: *최소 권장 자본:*\n"
            f"• 3x: ${config.MIN_CAPITAL_3X:,.0f} (₩{config.MIN_CAPITAL_3X * rate:,.0f})\n"
            f"• 4x: ${config.MIN_CAPITAL_4X:,.0f} (₩{config.MIN_CAPITAL_4X * rate:,.0f})\n\n"
            f":briefcase: *현재 포지션:* {position}\n"
            f":currency_exchange: *환율:* {rate:,.0f} KRW/USD\n\n"
            f"_실제 잔고는 IBKR에서 확인하세요_"
        )

    def get_rate(self) -> str:
        """Get exchange rate info."""
        rate = config.USD_KRW_RATE

        # 예시 금액 환산
        examples = [1000, 10000, 50000, 100000]

        lines = [f"*USD/KRW 환율*\n\n:currency_exchange: *{rate:,.0f}* KRW/USD\n"]
        lines.append("\n*환산 예시:*")
        for usd in examples:
            krw = usd * rate
            lines.append(f"• ${usd:,} = ₩{krw:,.0f}")

        lines.append(f"\n_설정값 기준. 실시간 환율은 봇 실행 중에만 적용_")

        return "\n".join(lines)

    def get_sma_detail(self) -> str:
        """Get detailed SMA information."""
        status = self.notifier.get_current_status()

        if "error" in status:
            return f":warning: 오류: {status['error']}"

        price = status['qqq_price']
        sma200 = status['sma200']
        sma50 = status['sma50']

        # 각 SMA까지의 거리
        dist_200 = status['dist_200']
        dist_50 = status['dist_50']

        # SMA 간 거리
        sma_spread = (sma50 - sma200) / sma200 * 100

        # 골든크로스/데드크로스 상태
        if sma50 > sma200:
            cross_status = ":chart_with_upwards_trend: 골든크로스 (SMA50 > SMA200)"
        else:
            cross_status = ":chart_with_downwards_trend: 데드크로스 (SMA50 < SMA200)"

        return (
            f"*SMA 상세 분석*\n\n"
            f"*QQQ:* ${price:.2f}\n\n"
            f"*SMA200:* ${sma200:.2f}\n"
            f"• 거리: {dist_200:+.2f}%\n"
            f"• {'위' if dist_200 > 0 else '아래'} {abs(dist_200):.2f}%\n\n"
            f"*SMA50:* ${sma50:.2f}\n"
            f"• 거리: {dist_50:+.2f}%\n"
            f"• {'위' if dist_50 > 0 else '아래'} {abs(dist_50):.2f}%\n\n"
            f"*SMA50-200 스프레드:* {sma_spread:+.2f}%\n"
            f"{cross_status}"
        )

    def get_config(self) -> str:
        """Get current configuration."""
        return (
            f"*전략 설정*\n\n"
            f":dart: *전략:* MNQ 3x + 이중SMA + GDX\n\n"
            f"*레버리지:*\n"
            f"• 기본: {config.TARGET_LEVERAGE_DEFAULT}x\n"
            f"• 고레버: {config.TARGET_LEVERAGE_HIGH}x (자본 ${config.LEVERAGE_4X_THRESHOLD:,.0f} 이상)\n"
            f"• 동적 전환: {'활성화' if config.ENABLE_DYNAMIC_LEVERAGE else '비활성화'}\n\n"
            f"*리밸런싱:*\n"
            f"• 밴드: ±{config.REBALANCE_BAND_PCT * 100:.0f}%\n"
            f"• 최소 임계값: {config.REBALANCE_MIN_THRESHOLD * 100:.1f}%\n\n"
            f"*SMA 기간:*\n"
            f"• 장기: {config.SMA_LONG_PERIOD}일\n"
            f"• 단기: {config.SMA_SHORT_PERIOD}일\n\n"
            f"*자산:*\n"
            f"• 롱: {config.MNQ_SYMBOL} (CME)\n"
            f"• 헤지: {config.HEDGE_SYMBOL}\n"
            f"• 시그널: {config.SIGNAL_SYMBOL}"
        )

    def get_risk(self) -> str:
        """Get risk assessment."""
        status = self.notifier.get_current_status()

        if "error" in status:
            return f":warning: 오류: {status['error']}"

        # 리스크 평가
        dist_200 = abs(status['dist_200'])
        dist_50 = abs(status['dist_50'])

        # 리스크 레벨 결정
        if status['above_200'] and status['above_50']:
            if dist_200 < 3:
                risk_level = ":warning: 중간"
                risk_desc = "SMA200 근처 - 하락 시 헤지 전환 가능성"
            elif dist_200 < 1:
                risk_level = ":rotating_light: 높음"
                risk_desc = "SMA200 매우 근접 - 변동성 주의"
            else:
                risk_level = ":white_check_mark: 낮음"
                risk_desc = "상승 추세 안정적"
        elif not status['above_200'] and not status['above_50']:
            risk_level = ":shield: 헤지 모드"
            risk_desc = "GDX 보유 중 - 하락장 보호"
        else:
            risk_level = ":hourglass: 전환 대기"
            risk_desc = "히스테리시스 구간 - 포지션 유지"

        # 변동성 기반 경고
        warnings = []
        if dist_200 < 2:
            warnings.append("• SMA200 근접: 추세 전환 가능성 주시")
        if dist_50 < 1:
            warnings.append("• SMA50 근접: 단기 변동성 주의")

        warning_text = "\n".join(warnings) if warnings else "• 특별한 경고 없음"

        return (
            f"*리스크 평가*\n\n"
            f"*리스크 레벨:* {risk_level}\n"
            f"*상태:* {risk_desc}\n\n"
            f"*SMA 거리:*\n"
            f"• SMA200: {status['dist_200']:+.2f}%\n"
            f"• SMA50: {status['dist_50']:+.2f}%\n\n"
            f"*경고:*\n{warning_text}"
        )

    def get_help(self) -> str:
        """Get help message."""
        return (
            "*MNQ Trader 명령어*\n\n"
            "*Slash 명령어:* `/ibkr [명령어]`\n\n"
            ":chart_with_upwards_trend: `status` - 시장 상태\n"
            ":briefcase: `position` - 현재 포지션\n"
            ":traffic_light: `signal` - 매매 신호\n"
            ":bank: `balance` - 계좌/잔고 정보\n"
            ":currency_exchange: `rate` - 환율 정보\n"
            ":bar_chart: `sma` - SMA 상세 분석\n"
            ":gear: `config` - 전략 설정\n"
            ":warning: `risk` - 리스크 평가\n"
            ":question: `help` - 이 메시지\n\n"
            "*예시:*\n"
            "• `/ibkr status`\n"
            "• `/ibkr balance`\n"
            "• `/ibkr risk`\n\n"
            "*한글 명령어 (DM/멘션):*\n"
            "상태, 포지션, 신호, 잔고, 환율, 이평, 설정, 위험, 도움말"
        )

    def run(self):
        """Start the chatbot."""
        if not BOLT_AVAILABLE:
            print("Error: slack-bolt 라이브러리가 설치되지 않았습니다.")
            print("  pip install slack-bolt")
            return

        if not self.app_token:
            print("=" * 60)
            print("Slack Socket Mode 설정이 필요합니다")
            print("=" * 60)
            print()
            print("1. https://api.slack.com/apps 에서 앱 선택")
            print("2. Settings > Socket Mode > Enable Socket Mode")
            print("3. 'connections:write' scope로 App-Level Token 생성")
            print("4. 환경변수 설정:")
            print("   export SLACK_APP_TOKEN=xapp-...")
            print()
            print("또는 config.py에 SLACK_APP_TOKEN 추가")
            print()
            print("-" * 60)
            print("테스트 모드: 명령어 결과 확인")
            print("-" * 60)
            print()
            print("[상태]")
            print(self.get_status())
            print()
            print("[포지션]")
            print(self.get_position())
            print()
            print("[신호]")
            print(self.get_signal())
            return

        if not self.bot_token:
            print("Error: SLACK_BOT_TOKEN이 설정되지 않았습니다.")
            return

        print("=" * 60)
        print("MNQ Trader Slack Chatbot")
        print("=" * 60)
        print("Starting Socket Mode...")

        self._handler = SocketModeHandler(self.app, self.app_token)
        self._running = True
        self._handler.start()

    def start_async(self) -> bool:
        """Start the chatbot in a background thread (non-blocking).

        Returns True if started successfully, False otherwise.
        """
        if not BOLT_AVAILABLE:
            print("[Chatbot] slack-bolt 라이브러리가 설치되지 않았습니다.")
            return False

        if not self.app_token or not self.bot_token:
            print("[Chatbot] Slack 토큰이 설정되지 않았습니다.")
            return False

        if self._running:
            print("[Chatbot] 이미 실행 중입니다.")
            return True

        def _run_handler():
            try:
                self._handler = SocketModeHandler(self.app, self.app_token)
                self._running = True
                print("[Chatbot] Slack 챗봇 시작됨 (백그라운드)")
                self._handler.start()
            except Exception as e:
                print(f"[Chatbot] 오류: {e}")
                self._running = False

        self._thread = threading.Thread(target=_run_handler, daemon=True)
        self._thread.start()
        return True

    def stop(self):
        """Stop the chatbot."""
        if self._handler and self._running:
            try:
                self._handler.close()
                print("[Chatbot] Slack 챗봇 종료됨")
            except Exception as e:
                print(f"[Chatbot] 종료 오류: {e}")
            finally:
                self._running = False
                self._handler = None

    @property
    def is_running(self) -> bool:
        """Check if chatbot is running."""
        return self._running


def interactive_mode():
    """Run in interactive CLI mode for testing."""
    bot = TradingChatbot()

    print("=" * 60)
    print("MNQ Trader - Interactive Mode")
    print("=" * 60)
    print("명령어: status, position, signal, help, quit")
    print()

    while True:
        try:
            cmd = input("> ").strip().lower()

            if cmd in ["quit", "exit", "q"]:
                print("Goodbye!")
                break
            elif cmd in ["status", "상태"]:
                print(bot.get_status().replace("*", "").replace("_", ""))
            elif cmd in ["position", "포지션"]:
                print(bot.get_position().replace("*", "").replace("_", ""))
            elif cmd in ["signal", "신호"]:
                print(bot.get_signal().replace("*", "").replace("_", ""))
            elif cmd in ["help", "도움말", ""]:
                print(bot.get_help().replace("*", "").replace("`", "").replace("_", ""))
            else:
                print("알 수 없는 명령어. 'help' 입력하세요.")
            print()
        except KeyboardInterrupt:
            print("\nGoodbye!")
            break
        except EOFError:
            break


def main():
    """Main entry point."""
    if len(sys.argv) > 1 and sys.argv[1] in ["-i", "--interactive"]:
        interactive_mode()
    else:
        bot = TradingChatbot()
        bot.run()


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""
Slack Chatbot for MNQ Trading System

Socket Mode를 사용한 실시간 명령어 처리

명령어:
- /status 또는 "상태" - 현재 시장 상태 확인
- /position 또는 "포지션" - 현재 포지션 확인
- /signal 또는 "신호" - 현재 신호 확인
- /help 또는 "도움말" - 명령어 목록

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

        # Slash commands (if configured in Slack App)
        @self.app.command("/status")
        def handle_status_command(ack, respond):
            ack()
            status = self.get_status()
            respond(status)

        @self.app.command("/position")
        def handle_position_command(ack, respond):
            ack()
            position = self.get_position()
            respond(position)

        @self.app.command("/signal")
        def handle_signal_command(ack, respond):
            ack()
            signal = self.get_signal()
            respond(signal)

    def _handle_command(self, text: str, say):
        """Handle text commands."""
        text = text.lower().strip()

        # Remove bot mention
        text = re.sub(r"<@[A-Z0-9]+>", "", text).strip()

        if any(cmd in text for cmd in ["status", "상태", "현황"]):
            say(self.get_status())
        elif any(cmd in text for cmd in ["position", "포지션", "보유"]):
            say(self.get_position())
        elif any(cmd in text for cmd in ["signal", "신호", "시그널"]):
            say(self.get_signal())
        elif any(cmd in text for cmd in ["help", "도움", "도움말", "명령어"]):
            say(self.get_help())
        else:
            say(self.get_help())

    def get_status(self) -> str:
        """Get current market status."""
        status = self.notifier.get_current_status()

        if "error" in status:
            return f":warning: 오류: {status['error']}"

        signal_emoji = ":chart_with_upwards_trend:" if status["signal"] == "LONG" else ":shield:"

        return (
            f"*현재 시장 상태*\n\n"
            f"*QQQ:* ${status['qqq_price']:.2f}\n"
            f"*SMA200:* ${status['sma200']:.2f} ({status['dist_200']:+.1f}%)\n"
            f"*SMA50:* ${status['sma50']:.2f} ({status['dist_50']:+.1f}%)\n\n"
            f"*신호:* {signal_emoji} {status['signal']}\n"
            f"{'• QQQ > SMA200' if status['above_200'] else '• QQQ < SMA200'}\n"
            f"{'• QQQ > SMA50' if status['above_50'] else '• QQQ < SMA50'}"
        )

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

    def get_help(self) -> str:
        """Get help message."""
        return (
            "*MNQ Trader 명령어*\n\n"
            ":chart_with_upwards_trend: `상태` 또는 `status` - 현재 시장 상태\n"
            ":briefcase: `포지션` 또는 `position` - 현재 보유 포지션\n"
            ":traffic_light: `신호` 또는 `signal` - 현재 매매 신호\n"
            ":question: `도움말` 또는 `help` - 이 메시지\n\n"
            "_DM으로 명령어를 보내거나, 채널에서 @멘션하세요_"
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

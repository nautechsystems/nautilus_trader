"""
Slack Bot for MNQ Trading System

Features:
- Trading notifications (buy/sell, position changes)
- Status commands
- Daily reports
"""

import json
import requests
from datetime import datetime
from typing import Optional
import yfinance as yf

from config import config


class SlackNotifier:
    """Slack notification handler."""

    def __init__(self):
        self.webhook_url = config.SLACK_WEBHOOK_URL
        self.bot_token = config.SLACK_BOT_TOKEN

    def send(self, message: str, emoji: str = "") -> bool:
        """Send a simple text message."""
        if not self.webhook_url:
            print(f"[Slack] {message}")
            return False

        payload = {"text": f"{emoji} {message}" if emoji else message}

        try:
            response = requests.post(
                self.webhook_url,
                json=payload,
                timeout=10
            )
            return response.status_code == 200
        except Exception as e:
            print(f"Slack error: {e}")
            return False

    def send_block(self, blocks: list, text: str = "") -> bool:
        """Send a rich block message."""
        if not self.webhook_url:
            print(f"[Slack] {text}")
            return False

        payload = {
            "text": text,
            "blocks": blocks
        }

        try:
            response = requests.post(
                self.webhook_url,
                json=payload,
                timeout=10
            )
            return response.status_code == 200
        except Exception as e:
            print(f"Slack error: {e}")
            return False

    # =========================================================================
    # Trading Notifications
    # =========================================================================

    def notify_startup(self, mode: str = "PAPER") -> bool:
        """Notify bot startup."""
        mode_kr = "모의투자" if mode == "PAPER" else "실거래"
        blocks = [
            {
                "type": "header",
                "text": {
                    "type": "plain_text",
                    "text": "MNQ 트레이더 시작"
                }
            },
            {
                "type": "section",
                "fields": [
                    {"type": "mrkdwn", "text": f"*모드:*\n{mode_kr}"},
                    {"type": "mrkdwn", "text": f"*전략:*\nMNQ 4x + 이중SMA + GDX"},
                ]
            },
            {
                "type": "section",
                "fields": [
                    {"type": "mrkdwn", "text": "*롱:*\nMNQ (4x)"},
                    {"type": "mrkdwn", "text": "*헤지:*\nGDX"},
                ]
            },
            {
                "type": "context",
                "elements": [
                    {"type": "mrkdwn", "text": f"시작 시간: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}"}
                ]
            }
        ]
        return self.send_block(blocks, "MNQ 트레이더 시작")

    def notify_signal(self, signal: str, qqq_price: float, sma200: float, sma50: float) -> bool:
        """Notify signal change."""
        dist_200 = (qqq_price - sma200) / sma200 * 100
        dist_50 = (qqq_price - sma50) / sma50 * 100

        signal_kr = {"LONG": "롱", "HEDGE": "헤지", "HOLD": "유지"}.get(signal, signal)

        if signal == "LONG":
            emoji = ":chart_with_upwards_trend:"
        else:
            emoji = ":shield:"

        blocks = [
            {
                "type": "header",
                "text": {
                    "type": "plain_text",
                    "text": f"{emoji} 신호: {signal_kr}"
                }
            },
            {
                "type": "section",
                "fields": [
                    {"type": "mrkdwn", "text": f"*QQQ:*\n${qqq_price:.2f}"},
                    {"type": "mrkdwn", "text": f"*신호:*\n{signal_kr}"},
                ]
            },
            {
                "type": "section",
                "fields": [
                    {"type": "mrkdwn", "text": f"*SMA200:*\n${sma200:.2f} ({dist_200:+.1f}%)"},
                    {"type": "mrkdwn", "text": f"*SMA50:*\n${sma50:.2f} ({dist_50:+.1f}%)"},
                ]
            }
        ]
        return self.send_block(blocks, f"신호: {signal_kr}")

    def notify_position_change(
        self,
        action: str,
        symbol: str,
        quantity: int,
        price: float,
        from_position: Optional[str] = None,
        to_position: Optional[str] = None
    ) -> bool:
        """Notify position change."""
        if action == "BUY":
            emoji = ":green_circle:"
            action_text = "매수"
        else:
            emoji = ":red_circle:"
            action_text = "매도"

        value = quantity * price

        blocks = [
            {
                "type": "header",
                "text": {
                    "type": "plain_text",
                    "text": f"{emoji} {action_text}: {symbol}"
                }
            },
            {
                "type": "section",
                "fields": [
                    {"type": "mrkdwn", "text": f"*종목:*\n{symbol}"},
                    {"type": "mrkdwn", "text": f"*주문:*\n{action_text}"},
                ]
            },
            {
                "type": "section",
                "fields": [
                    {"type": "mrkdwn", "text": f"*수량:*\n{quantity:,}"},
                    {"type": "mrkdwn", "text": f"*가격:*\n${price:.2f}"},
                ]
            },
            {
                "type": "section",
                "fields": [
                    {"type": "mrkdwn", "text": f"*금액:*\n${value:,.0f}"},
                ]
            }
        ]

        if from_position and to_position:
            from_kr = {"LONG": "롱", "HEDGE": "헤지"}.get(from_position, from_position)
            to_kr = {"LONG": "롱", "HEDGE": "헤지"}.get(to_position, to_position)
            blocks.append({
                "type": "context",
                "elements": [
                    {"type": "mrkdwn", "text": f"포지션: {from_kr} → {to_kr}"}
                ]
            })

        return self.send_block(blocks, f"{action_text} {symbol}")

    def notify_rebalance(self, symbol: str, diff: int, price: float) -> bool:
        """Notify rebalancing."""
        if diff > 0:
            emoji = ":arrow_up:"
            action = "추가 매수"
        else:
            emoji = ":arrow_down:"
            action = "일부 매도"

        return self.send(
            f"{emoji} *리밸런싱* {symbol}: {action} {abs(diff)}주 @ ${price:.2f}",
            emoji=""
        )

    def notify_error(self, error: str) -> bool:
        """Notify error."""
        return self.send(f":rotating_light: *오류*: {error}")

    def notify_daily_report(
        self,
        position: str,
        account_value: float,
        daily_pnl: float,
        daily_pnl_pct: float
    ) -> bool:
        """Send daily report."""
        if daily_pnl >= 0:
            pnl_emoji = ":chart_with_upwards_trend:"
        else:
            pnl_emoji = ":chart_with_downwards_trend:"

        position_kr = {"LONG": "롱", "HEDGE": "헤지"}.get(position, position)

        blocks = [
            {
                "type": "header",
                "text": {
                    "type": "plain_text",
                    "text": "일일 리포트"
                }
            },
            {
                "type": "section",
                "fields": [
                    {"type": "mrkdwn", "text": f"*포지션:*\n{position_kr}"},
                    {"type": "mrkdwn", "text": f"*계좌:*\n${account_value:,.0f}"},
                ]
            },
            {
                "type": "section",
                "fields": [
                    {"type": "mrkdwn", "text": f"*일일 손익:*\n{pnl_emoji} ${daily_pnl:+,.0f} ({daily_pnl_pct:+.2f}%)"},
                ]
            },
            {
                "type": "context",
                "elements": [
                    {"type": "mrkdwn", "text": f"리포트 생성: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}"}
                ]
            }
        ]
        return self.send_block(blocks, "일일 리포트")

    # =========================================================================
    # Status Commands
    # =========================================================================

    def get_current_status(self) -> dict:
        """Get current market status."""
        try:
            qqq = yf.download("QQQ", period="1y", progress=False)
            if len(qqq) < 200:
                return {"error": "Insufficient data"}

            close = qqq['Close']
            if hasattr(close, 'columns'):
                close = close.iloc[:, 0]

            price = float(close.iloc[-1])
            sma200 = float(close.iloc[-200:].mean())
            sma50 = float(close.iloc[-50:].mean())

            above_200 = price > sma200
            above_50 = price > sma50

            if above_200 and above_50:
                signal = "LONG"
            elif not above_200 and not above_50:
                signal = "HEDGE"
            else:
                signal = "HOLD"

            return {
                "qqq_price": price,
                "sma200": sma200,
                "sma50": sma50,
                "above_200": above_200,
                "above_50": above_50,
                "signal": signal,
                "dist_200": (price - sma200) / sma200 * 100,
                "dist_50": (price - sma50) / sma50 * 100,
            }
        except Exception as e:
            return {"error": str(e)}

    def send_status(self) -> bool:
        """Send current status to Slack."""
        status = self.get_current_status()

        if "error" in status:
            return self.notify_error(status["error"])

        return self.notify_signal(
            status["signal"],
            status["qqq_price"],
            status["sma200"],
            status["sma50"]
        )


# =========================================================================
# Test
# =========================================================================

def test_slack():
    """Test Slack notifications."""
    notifier = SlackNotifier()

    print("Testing Slack notifications...")

    # Test startup
    notifier.notify_startup("PAPER")

    # Test signal
    status = notifier.get_current_status()
    if "error" not in status:
        notifier.notify_signal(
            status["signal"],
            status["qqq_price"],
            status["sma200"],
            status["sma50"]
        )

    print("Done!")


if __name__ == "__main__":
    test_slack()

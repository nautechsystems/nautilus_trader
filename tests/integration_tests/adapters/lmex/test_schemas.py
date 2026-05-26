# -------------------------------------------------------------------------------------------------
#  LMEX NautilusTrader Adapter — Tests
# -------------------------------------------------------------------------------------------------

"""
Tests for msgspec schema decode correctness.

Uses real API response fixtures (no live calls required).
"""

from __future__ import annotations

import json

import msgspec
import pytest


class TestMarketSchemas:
    """Tests for REST market response schemas."""

    def test_server_time_decode(self, time_fixture: bytes) -> None:
        """Server time decodes correctly."""
        from nautilus_trader.adapters.lmex.schemas.market import LmexServerTime

        dec = msgspec.json.Decoder(LmexServerTime)
        result = dec.decode(time_fixture)
        assert isinstance(result.iso, str)
        assert isinstance(result.epoch, int)
        assert result.epoch > 1_700_000_000  # sanity: after 2023

    def test_orderbook_decode(self, orderbook_fixture: bytes) -> None:
        """Order book decodes with correct bid/ask structure."""
        from nautilus_trader.adapters.lmex.schemas.market import LmexOrderBook

        dec = msgspec.json.Decoder(LmexOrderBook)
        result = dec.decode(orderbook_fixture)

        assert result.symbol == "BTC-USD"
        assert len(result.buyQuote) > 0
        assert len(result.sellQuote) > 0

        # Prices/sizes are strings in the REST response
        entry = result.buyQuote[0]
        assert isinstance(entry.price, str)
        assert isinstance(entry.size, str)
        assert float(entry.price) > 0
        assert float(entry.size) > 0

    def test_orderbook_bid_ask_ordering(self, orderbook_fixture: bytes) -> None:
        """Bids should be sorted best-first (highest price), asks lowest-first."""
        from nautilus_trader.adapters.lmex.schemas.market import LmexOrderBook

        dec = msgspec.json.Decoder(LmexOrderBook)
        result = dec.decode(orderbook_fixture)

        if len(result.buyQuote) >= 2:
            assert float(result.buyQuote[0].price) >= float(result.buyQuote[1].price)
        if len(result.sellQuote) >= 2:
            assert float(result.sellQuote[0].price) <= float(result.sellQuote[1].price)

    def test_trades_decode(self, trades_fixture: bytes) -> None:
        """Trade list decodes with required fields."""
        from nautilus_trader.adapters.lmex.schemas.market import LmexTrade

        dec = msgspec.json.Decoder(list[LmexTrade])
        result = dec.decode(trades_fixture)

        assert len(result) > 0
        trade = result[0]
        assert isinstance(trade.price, float)
        assert isinstance(trade.size, float)
        assert trade.side in ("BUY", "SELL")
        assert trade.symbol == "BTC-USD"
        assert isinstance(trade.serialId, int)
        assert trade.timestamp > 1_700_000_000_000  # ms since 2023

    def test_market_summary_decode(self, market_summary_fixture: bytes) -> None:
        """Market summary decodes with instrument fields."""
        from nautilus_trader.adapters.lmex.schemas.market import LmexMarketSummary

        dec = msgspec.json.Decoder(list[LmexMarketSummary])
        result = dec.decode(market_summary_fixture)

        assert len(result) > 0
        btc = next((s for s in result if s.symbol == "BTC-USD"), None)
        assert btc is not None
        assert btc.base == "BTC"
        assert btc.quote == "USD"
        assert btc.active is True
        assert btc.minPriceIncrement > 0
        assert btc.minSizeIncrement > 0
        assert btc.minOrderSize > 0
        assert btc.maxOrderSize > 0


class TestWsSchemas:
    """Tests for WebSocket message schemas."""

    def test_trade_message_decode(self, ws_trade_msg: bytes) -> None:
        """Trade WS message decodes correctly."""
        from nautilus_trader.adapters.lmex.schemas.ws import LmexWsTradeMsg

        dec = msgspec.json.Decoder(LmexWsTradeMsg)
        result = dec.decode(ws_trade_msg)

        assert result.topic == "tradeHistoryApi:BTC-USD"
        assert len(result.data) == 2

        first = result.data[0]
        assert first.symbol == "BTC-USD"
        assert first.side == "SELL"
        assert first.size == 0.0145
        assert first.price == 76653.1
        assert first.tradeId == 31626447
        assert first.timestamp == 1779786372223

    def test_subscribe_ack_decode(self, ws_subscribe_ack: bytes) -> None:
        """Subscribe ack decodes correctly."""
        from nautilus_trader.adapters.lmex.schemas.ws import LmexWsSubscribeAck

        dec = msgspec.json.Decoder(LmexWsSubscribeAck)
        result = dec.decode(ws_subscribe_ack)

        assert result.event == "subscribe"
        assert "tradeHistoryApi:BTC-USD" in result.channel

    def test_order_fill_event_decode(self, ws_order_fill: bytes) -> None:
        """Order fill event decodes with all required fields."""
        from nautilus_trader.adapters.lmex.schemas.ws import LmexWsOrderEventMsg

        dec = msgspec.json.Decoder(LmexWsOrderEventMsg)
        result = dec.decode(ws_order_fill)

        assert result.topic == "notificationsApi"
        assert len(result.data) == 1

        event = result.data[0]
        assert event.symbol == "BTC-USD"
        assert event.orderId == 987654321
        assert event.clOrderId == "my-order-001"
        assert event.status == 4   # ORDER_FULLY_TRANSACTED
        assert event.side == "BUY"
        assert event.avgFillPrice == 76501.5
        assert event.feeAmount == 0.765
        assert event.feeCurrency == "USD"

    def test_order_cancel_event_decode(self, ws_order_cancel: bytes) -> None:
        """Order cancel event decodes correctly."""
        from nautilus_trader.adapters.lmex.schemas.ws import LmexWsOrderEventMsg

        dec = msgspec.json.Decoder(LmexWsOrderEventMsg)
        result = dec.decode(ws_order_cancel)

        event = result.data[0]
        assert event.status == 6   # ORDER_CANCELLED
        assert event.filledSize == 0.0

    def test_envelope_dispatch_by_topic(self, ws_trade_msg: bytes) -> None:
        """Generic envelope correctly identifies topic vs event messages."""
        from nautilus_trader.adapters.lmex.schemas.ws import LmexWsMsg

        dec = msgspec.json.Decoder(LmexWsMsg)
        result = dec.decode(ws_trade_msg)

        assert result.topic is not None
        assert result.event is None
        assert result.topic.startswith("tradeHistoryApi:")

    def test_envelope_dispatch_by_event(self, ws_subscribe_ack: bytes) -> None:
        """Generic envelope identifies control messages by event field."""
        from nautilus_trader.adapters.lmex.schemas.ws import LmexWsMsg

        dec = msgspec.json.Decoder(LmexWsMsg)
        result = dec.decode(ws_subscribe_ack)

        assert result.event == "subscribe"
        assert result.topic is None

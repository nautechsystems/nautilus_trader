"""
Event Emitter Actor for bot-folio.

Subscribes to trading events within Nautilus and publishes them to Redis
for the backend to persist orders, fills, and positions.
"""
import json
import os
from datetime import datetime, timezone
from decimal import Decimal
from typing import Any

import redis

from nautilus_trader.common.actor import Actor
from nautilus_trader.config import ActorConfig
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.events import OrderCanceled
from nautilus_trader.model.events import OrderAccepted
from nautilus_trader.model.events import PositionChanged
from nautilus_trader.model.events import PositionClosed
from nautilus_trader.model.events import PositionOpened
from nautilus_trader.model.position import Position


class EventEmitterConfig(ActorConfig, frozen=True):
    """Configuration for the EventEmitter actor."""

    bot_id: str = ""
    redis_url: str = "redis://localhost:6379"


class EventEmitter(Actor):
    """
    Actor that emits trading events to Redis for backend consumption.

    Subscribes to:
    - Order events (filled, rejected, canceled)
    - Position events (opened, changed, closed)

    Publishes to Redis channel: engine:events:{bot_id}
    """

    def __init__(self, config: EventEmitterConfig) -> None:
        super().__init__(config)
        self._bot_id = config.bot_id or os.environ.get("BOTFOLIO_BOT_ID", "")
        self._redis_url = config.redis_url or os.environ.get("REDIS_URL", "redis://localhost:6379")
        self._redis: redis.Redis | None = None
        self._channel = f"engine:events:{self._bot_id}"

    def on_start(self) -> None:
        """Connect to Redis and subscribe to trading events."""
        if not self._bot_id:
            self._log.warning("No bot_id configured, events will not be emitted")
            return

        try:
            self._redis = redis.from_url(self._redis_url, decode_responses=True)
            self._redis.ping()
            self._log.info(f"Connected to Redis, publishing to {self._channel}")
        except Exception as e:
            self._log.error(f"Failed to connect to Redis: {e}")
            self._redis = None
            return

        # Subscribe to all order and position events via message bus
        self.msgbus.subscribe(topic="events.order.*", handler=self._handle_order_event)
        self.msgbus.subscribe(topic="events.position.*", handler=self._handle_position_event)

        self._log.info("EventEmitter started, subscribed to order and position events")

    def on_stop(self) -> None:
        """Clean up Redis connection."""
        if self._redis:
            try:
                self._redis.close()
            except Exception:
                pass
            self._redis = None
        self._log.info("EventEmitter stopped")

    def _publish(self, event_type: str, data: dict[str, Any]) -> None:
        """Publish an event to Redis."""
        if not self._redis:
            return

        now = datetime.now(timezone.utc)
        ts_ns = int(now.timestamp() * 1_000_000_000)

        envelope = {
            "type": event_type,
            "ts": now.isoformat().replace("+00:00", "Z"),
            "ts_ns": ts_ns,
            "bot_id": self._bot_id,
            "data": data,
        }

        try:
            self._redis.publish(self._channel, json.dumps(envelope, default=self._json_default))
            self._log.info(f"Published {event_type} event to {self._channel}")
        except Exception as e:
            self._log.error(f"Failed to publish event: {e}")

    @staticmethod
    def _json_default(obj: Any) -> Any:
        """JSON serializer for objects not serializable by default."""
        if isinstance(obj, Decimal):
            return str(obj)
        if hasattr(obj, "to_str"):
            return obj.to_str()
        if hasattr(obj, "__str__"):
            return str(obj)
        raise TypeError(f"Object of type {type(obj)} is not JSON serializable")

    def _handle_order_event(self, event: Any) -> None:
        """Handle order events from the message bus."""
        self._log.info(f"Received order event: {type(event).__name__}")
        if isinstance(event, OrderFilled):
            self._on_order_filled(event)
        elif isinstance(event, OrderAccepted):
            self._on_order_accepted(event)
        elif isinstance(event, OrderRejected):
            self._on_order_rejected(event)
        elif isinstance(event, OrderCanceled):
            self._on_order_canceled(event)

    def _handle_position_event(self, event: Any) -> None:
        """Handle position events from the message bus."""
        if isinstance(event, (PositionOpened, PositionChanged, PositionClosed)):
            self._on_position_event(event)

    def _on_order_accepted(self, event: OrderAccepted) -> None:
        """Handle order accepted event."""
        self._publish("order_accepted", {
            "client_order_id": str(event.client_order_id),
            "venue_order_id": str(event.venue_order_id) if event.venue_order_id else None,
            "instrument_id": str(event.instrument_id),
            "strategy_id": str(event.strategy_id),
            "account_id": str(event.account_id),
            "event_id": str(event.event_id),
            "ts_event": event.ts_event,
        })

    def _on_order_filled(self, event: OrderFilled) -> None:
        """Handle order filled event."""
        self._publish("order_filled", {
            "client_order_id": str(event.client_order_id),
            "venue_order_id": str(event.venue_order_id),
            "trade_id": str(event.trade_id),
            "instrument_id": str(event.instrument_id),
            "strategy_id": str(event.strategy_id),
            "account_id": str(event.account_id),
            "order_side": event.order_side.name,
            "order_type": event.order_type.name,
            "last_qty": str(event.last_qty),
            "last_px": str(event.last_px),
            "currency": str(event.currency),
            "liquidity_side": event.liquidity_side.name,
            "commission": str(event.commission) if event.commission else None,
            "position_id": str(event.position_id) if event.position_id else None,
            "event_id": str(event.event_id),
            "ts_event": event.ts_event,
        })

    def _on_order_rejected(self, event: OrderRejected) -> None:
        """Handle order rejected event."""
        self._publish("order_rejected", {
            "client_order_id": str(event.client_order_id),
            "instrument_id": str(event.instrument_id),
            "strategy_id": str(event.strategy_id),
            "account_id": str(event.account_id),
            "reason": event.reason,
            "event_id": str(event.event_id),
            "ts_event": event.ts_event,
        })

    def _on_order_canceled(self, event: OrderCanceled) -> None:
        """Handle order canceled event."""
        self._publish("order_canceled", {
            "client_order_id": str(event.client_order_id),
            "venue_order_id": str(event.venue_order_id) if event.venue_order_id else None,
            "instrument_id": str(event.instrument_id),
            "strategy_id": str(event.strategy_id),
            "account_id": str(event.account_id),
            "event_id": str(event.event_id),
            "ts_event": event.ts_event,
        })

    def _on_position_event(self, event: PositionOpened | PositionChanged | PositionClosed) -> None:
        """Handle position events."""
        position: Position | None = self.cache.position(event.position_id)
        if not position:
            return

        event_type = {
            PositionOpened: "position_opened",
            PositionChanged: "position_changed",
            PositionClosed: "position_closed",
        }.get(type(event), "position")

        self._publish(event_type, {
            "position_id": str(event.position_id),
            "instrument_id": str(event.instrument_id),
            "strategy_id": str(event.strategy_id),
            "account_id": str(position.account_id),
            "side": position.side.name,
            "quantity": str(position.quantity),
            "avg_px_open": str(position.avg_px_open),
            "avg_px_close": str(position.avg_px_close) if position.avg_px_close else None,
            "realized_pnl": str(position.realized_pnl) if position.realized_pnl else None,
            "unrealized_pnl": str(position.unrealized_pnl(position.avg_px_open)) if position.is_open else None,
            "ts_opened": position.ts_opened,
            "ts_closed": position.ts_closed if position.is_closed else None,
            "event_id": str(event.event_id),
            "ts_event": event.ts_event,
        })


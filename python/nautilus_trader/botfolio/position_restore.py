"""
Position restoration for bot-folio position isolation.

When multiple bots share the same Alpaca account, each bot needs to track
only its own positions. This module restores a bot's positions from the
backend database into the Nautilus cache on startup.
"""
import json
import os
from decimal import Decimal

from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import Logger
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position


def restore_positions_from_env(
    cache: Cache,
    account_id: AccountId,
    trader_id: TraderId,
    strategy_id: StrategyId,
    venue: str,
    logger: Logger | None = None,
) -> int:
    """
    Restore bot positions from BOTFOLIO_POSITIONS environment variable.

    This function reads positions serialized by the backend and creates
    Position objects in the Nautilus cache. This ensures the bot sees
    only its own positions, not the aggregate Alpaca account positions.

    Parameters
    ----------
    cache : Cache
        The Nautilus cache to populate with positions.
    account_id : AccountId
        The account ID for the positions.
    trader_id : TraderId
        The trader ID for the positions.
    strategy_id : StrategyId
        The strategy ID for the positions.
    venue : str
        The venue string (e.g., "ALPACA") for instrument IDs.
    logger : Logger, optional
        Logger for diagnostic output.

    Returns
    -------
    int
        Number of positions restored.

    """
    positions_json = os.environ.get("BOTFOLIO_POSITIONS", "[]")

    try:
        positions_data = json.loads(positions_json)
    except json.JSONDecodeError:
        if logger:
            logger.warning("Failed to parse BOTFOLIO_POSITIONS JSON")
        return 0

    if not positions_data:
        if logger:
            logger.info("No positions to restore")
        return 0

    restored_count = 0

    for pos_data in positions_data:
        try:
            symbol = pos_data.get("symbol")
            quantity = Decimal(str(pos_data.get("quantity", 0)))
            avg_price = Decimal(str(pos_data.get("averagePrice", 0)))

            if abs(quantity) < Decimal("0.00000001"):
                continue  # Skip zero positions

            instrument_id = InstrumentId.from_str(f"{symbol}.{venue}")

            # Get the instrument from cache to determine precision
            instrument = cache.instrument(instrument_id)
            if instrument is None:
                if logger:
                    logger.warning(f"Instrument {instrument_id} not in cache, skipping position restore")
                continue

            # Determine order side from quantity sign
            order_side = OrderSide.BUY if quantity > 0 else OrderSide.SELL
            abs_quantity = abs(quantity)

            # Create a synthetic fill event to establish the position
            # This mimics how Nautilus creates positions from fills
            fill = OrderFilled(
                trader_id=trader_id,
                strategy_id=strategy_id,
                instrument_id=instrument_id,
                client_order_id=ClientOrderId(f"RESTORE-{symbol}-{UUID4().value[:8]}"),
                venue_order_id=VenueOrderId(f"RESTORE-{UUID4().value[:8]}"),
                account_id=account_id,
                trade_id=TradeId(f"RESTORE-{UUID4().value[:8]}"),
                order_side=order_side,
                order_type=None,  # Not applicable for restored positions
                last_qty=Quantity(abs_quantity, instrument.size_precision),
                last_px=Price(avg_price, instrument.price_precision),
                currency=Currency.from_str("USD"),
                liquidity_side=None,
                event_id=UUID4(),
                ts_event=0,
                ts_init=0,
                reconciliation=True,  # Mark as reconciliation to avoid triggering events
            )

            # Create position from the fill
            position = Position(instrument=instrument, fill=fill)

            # Add to cache with NETTING OMS type (positions are per-instrument-per-strategy)
            cache.add_position(position, OmsType.NETTING)

            if logger:
                logger.info(
                    f"Restored position: {symbol} qty={quantity} avg_px={avg_price}"
                )

            restored_count += 1

        except Exception as e:
            if logger:
                logger.error(f"Failed to restore position {pos_data}: {e}")
            continue

    if logger:
        logger.info(f"Position isolation: restored {restored_count} position(s)")

    return restored_count


# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------
"""
Helper functions for converting Nautilus orders to Hyperliquid format.
"""

import json

from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.orders import Order


def extract_asset_from_symbol(symbol: str) -> str:
    """
    Extract asset ID from instrument symbol.
    """
    # Remove venue suffix if present
    if "." in symbol:
        symbol = symbol.split(".")[0]
    # Remove -PERP or -USD suffix
    if symbol.endswith("-PERP"):
        return symbol[:-5]
    if symbol.endswith("-USD"):
        return symbol[:-4]
    return symbol


def order_to_hyperliquid_json(order: Order) -> str:
    """
    Convert a Nautilus order to Hyperliquid order format JSON.

    Parameters
    ----------
    order : Order
        The Nautilus order to convert.

    Returns
    -------
    str
        JSON string representing the Hyperliquid order request.

    """
    # Extract asset from instrument symbol
    asset = extract_asset_from_symbol(str(order.instrument_id.symbol))

    # Determine order side
    is_buy = order.side == OrderSide.BUY

    # Get price (use 0 for market orders as placeholder)
    if order.price is not None:
        price = str(order.price)
    else:
        price = "0"

    # Get size
    size = str(order.quantity)

    # Determine order kind and time-in-force
    if order.order_type == OrderType.MARKET:
        # Market orders are implemented as limit IOC in Hyperliquid
        order_kind = {
            "limit": {
                "tif": "Ioc",
            },
        }
    elif order.order_type == OrderType.LIMIT:
        # Map Nautilus TIF to Hyperliquid TIF
        if order.time_in_force == TimeInForce.GTC:
            tif = "Gtc"
        elif order.time_in_force == TimeInForce.IOC:
            tif = "Ioc"
        elif order.time_in_force == TimeInForce.FOK:
            tif = "Alo"  # Alo = All-or-none (Hyperliquid TIF)
        else:
            tif = "Gtc"  # Default to GTC

        order_kind = {
            "limit": {
                "tif": tif,
            },
        }
    else:
        # For other order types, default to limit GTC
        order_kind = {
            "limit": {
                "tif": "Gtc",
            },
        }

    # Build the order request
    order_request = {
        "asset": asset,
        "isBuy": is_buy,
        "limitPx": price,
        "sz": size,
        "reduceOnly": order.is_reduce_only,
        "orderType": order_kind,
        "cloid": str(order.client_order_id),
    }

    # Return as JSON array (Hyperliquid expects an array of orders)
    return json.dumps([order_request])


def orders_to_hyperliquid_json(orders: list[Order]) -> str:
    """
    Convert multiple Nautilus orders to Hyperliquid orders array JSON.

    Parameters
    ----------
    orders : list[Order]
        List of Nautilus orders to convert.

    Returns
    -------
    str
        JSON string representing the Hyperliquid orders array.

    """
    order_requests = []

    for order in orders:
        # Extract asset from instrument symbol
        asset = extract_asset_from_symbol(str(order.instrument_id.symbol))

        # Determine order side
        is_buy = order.side == OrderSide.BUY

        # Get price (use 0 for market orders as placeholder)
        if order.price is not None:
            price = str(order.price)
        else:
            price = "0"

        # Get size
        size = str(order.quantity)

        # Determine order kind and time-in-force
        if order.order_type == OrderType.MARKET:
            order_kind = {
                "limit": {
                    "tif": "Ioc",
                },
            }
        elif order.order_type == OrderType.LIMIT:
            if order.time_in_force == TimeInForce.GTC:
                tif = "Gtc"
            elif order.time_in_force == TimeInForce.IOC:
                tif = "Ioc"
            elif order.time_in_force == TimeInForce.FOK:
                tif = "Alo"  # Alo = All-or-none (Hyperliquid TIF)
            else:
                tif = "Gtc"

            order_kind = {
                "limit": {
                    "tif": tif,
                },
            }
        else:
            order_kind = {
                "limit": {
                    "tif": "Gtc",
                },
            }

        # Build the order request
        order_request = {
            "asset": asset,
            "isBuy": is_buy,
            "limitPx": price,
            "sz": size,
            "reduceOnly": order.is_reduce_only,
            "orderType": order_kind,
            "cloid": str(order.client_order_id),
        }

        order_requests.append(order_request)

    return json.dumps(order_requests)

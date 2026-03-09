# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
Order action persistence package.
"""

from nautilus_trader.persistence.orders.actor import OrderActionPersistenceActor
from nautilus_trader.persistence.orders.actor import order_event_to_row
from nautilus_trader.persistence.orders.config import DEFAULT_ORDER_ACTION_EVENT_TYPES
from nautilus_trader.persistence.orders.config import OrderActionPersistenceActorConfig
from nautilus_trader.persistence.orders.schema import INSERT_ORDER_ACTION_SQL
from nautilus_trader.persistence.orders.schema import ORDER_ACTION_SCHEMA_SQL
from nautilus_trader.persistence.orders.sqlite import OrderActionRow
from nautilus_trader.persistence.orders.sqlite import connect
from nautilus_trader.persistence.orders.sqlite import ensure_schema
from nautilus_trader.persistence.orders.sqlite import insert_many

__all__ = [
    "DEFAULT_ORDER_ACTION_EVENT_TYPES",
    "INSERT_ORDER_ACTION_SQL",
    "OrderActionPersistenceActor",
    "OrderActionPersistenceActorConfig",
    "ORDER_ACTION_SCHEMA_SQL",
    "OrderActionRow",
    "connect",
    "ensure_schema",
    "insert_many",
    "order_event_to_row",
]

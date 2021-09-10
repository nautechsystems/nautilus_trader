# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.events.position import PositionEvent
from nautilus_trader.model.objects import Money


def try_float(x):
    if x == "None" or x is None:
        return
    return float(x)


def serialize(event: PositionEvent):
    data = {k: v for k, v in event.to_dict(event).items() if k not in ("order_fill",)}
    caster = {
        "net_qty": float,
        "quantity": float,
        "peak_qty": float,
        "avg_px_open": float,
        "avg_px_close": try_float,
    }
    values = {k: caster[k](v) if k in caster else v for k, v in data.items()}  # type: ignore
    values["realized_pnl"] = Money.from_str(values["realized_pnl"]).as_double()
    return values


def deserialize(_):
    raise NotImplementedError()  # pragma: no cover

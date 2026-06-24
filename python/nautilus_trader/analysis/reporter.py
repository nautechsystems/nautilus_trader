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
Pandas report generation for backtest results.

`pandas` is an optional dependency (`pip install pandas`); it is imported lazily so that
`nautilus_trader.analysis` can be imported without it.

"""

from __future__ import annotations

from typing import TYPE_CHECKING

from nautilus_trader.model import OrderFilled


if TYPE_CHECKING:
    import pandas as pd


def _require_pandas() -> None:
    try:
        import pandas as pd  # noqa: F401 (presence check)
    except ImportError as e:
        raise ImportError(
            "pandas is required for report generation; install it with `pip install pandas`",
        ) from e


def _ns_to_dt(ts: int) -> pd.Timestamp:
    import pandas as pd

    return pd.Timestamp(ts, tz="UTC")


class ReportProvider:
    """
    Provides various portfolio analysis reports.
    """

    @staticmethod
    def generate_orders_report(orders: list) -> pd.DataFrame:
        _require_pandas()
        import pandas as pd

        if not orders:
            return pd.DataFrame()
        data = [o.to_dict() for o in orders]
        return pd.DataFrame(data=data).set_index("client_order_id").sort_index()

    @staticmethod
    def generate_order_fills_report(orders: list) -> pd.DataFrame:
        _require_pandas()
        import pandas as pd

        if not orders:
            return pd.DataFrame()
        dicts = [o.to_dict() for o in orders]
        filled = [d for d in dicts if float(d.get("filled_qty") or 0) > 0]
        if not filled:
            return pd.DataFrame()
        report = pd.DataFrame(data=filled).set_index("client_order_id").sort_index()
        report["ts_last"] = [_ns_to_dt(ts or 0) for ts in report["ts_last"]]
        report["ts_init"] = [_ns_to_dt(ts) for ts in report["ts_init"]]
        return report

    @staticmethod
    def generate_fills_report(orders: list) -> pd.DataFrame:
        _require_pandas()
        import pandas as pd

        if not orders:
            return pd.DataFrame()
        fills = [e.to_dict() for o in orders for e in o.events() if isinstance(e, OrderFilled)]
        if not fills:
            return pd.DataFrame()
        report = pd.DataFrame(data=fills).set_index("client_order_id").sort_index()
        report["ts_event"] = [_ns_to_dt(ts or 0) for ts in report["ts_event"]]
        report["ts_init"] = [_ns_to_dt(ts) for ts in report["ts_init"]]
        if "type" in report.columns:
            del report["type"]
        return report

    @staticmethod
    def generate_positions_report(
        positions: list,
        snapshots: list | None = None,
    ) -> pd.DataFrame:
        _require_pandas()
        import pandas as pd

        all_positions = list(positions)
        snapshot_ids: set[str] = set()

        if snapshots:
            all_positions.extend(snapshots)
            snapshot_ids = {str(p.id) for p in snapshots}
        if not all_positions:
            return pd.DataFrame()
        data = [p.to_dict() for p in all_positions]
        sort = ["ts_opened", "ts_closed", "position_id"]
        report = pd.DataFrame(data=data).set_index("position_id").sort_values(sort)
        for col in ("signed_qty", "quote_currency", "base_currency", "settlement_currency"):
            if col in report.columns:
                del report[col]
        report["ts_opened"] = [_ns_to_dt(ts) for ts in report["ts_opened"]]
        report["ts_closed"] = [
            _ns_to_dt(ts) if ts is not None and not pd.isna(ts) else pd.NA
            for ts in report["ts_closed"]
        ]
        report["is_snapshot"] = report.index.isin(snapshot_ids)
        return report

    @staticmethod
    def generate_account_report(account) -> pd.DataFrame:
        _require_pandas()
        import pandas as pd

        states = account.events
        if not states:
            return pd.DataFrame()
        account_states = [s.to_dict() for s in states]
        balances = [
            {**balance, **state}
            for state in account_states
            for balance in state.pop("balances", [])
        ]

        if not balances:
            return pd.DataFrame()
        report = pd.DataFrame(data=balances).set_index("ts_event").sort_index()
        report.index = [_ns_to_dt(ts) for ts in report.index]
        for col in ("ts_init", "type", "event_id"):
            if col in report.columns:
                del report[col]
        return report

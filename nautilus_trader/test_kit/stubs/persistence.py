# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

import pandas as pd

from nautilus_trader.core.datetime import maybe_dt_to_unix_nanos
from nautilus_trader.model.currency import Currency
from nautilus_trader.serialization.arrow.serializer import register_parquet
from nautilus_trader.test_kit.mocks.data import NewsEventData
from nautilus_trader.trading.filters import NewsImpact


class TestPersistenceStubs:
    @staticmethod
    def setup_news_event_persistence():
        import pyarrow as pa

        def _news_event_to_dict(self):
            return {
                "name": self.name,
                "impact": self.impact.name,
                "currency": self.currency.code,
                "ts_event": self.ts_event,
                "ts_init": self.ts_init,
            }

        def _news_event_from_dict(data):
            data.update(
                {
                    "impact": getattr(NewsImpact, data["impact"]),
                    "currency": Currency.from_str(data["currency"]),
                },
            )
            return NewsEventData(**data)

        register_parquet(
            cls=NewsEventData,
            serializer=_news_event_to_dict,
            deserializer=_news_event_from_dict,
            partition_keys=("currency",),
            schema=pa.schema(
                {
                    "name": pa.string(),
                    "impact": pa.string(),
                    "currency": pa.string(),
                    "ts_event": pa.uint64(),
                    "ts_init": pa.uint64(),
                },
            ),
            force=True,
        )

    @staticmethod
    def news_event_parser(df, state=None):
        for _, row in df.iterrows():
            yield NewsEventData(
                name=str(row["Name"]),
                impact=getattr(NewsImpact, row["Impact"]),
                currency=Currency.from_str(row["Currency"]),
                ts_event=maybe_dt_to_unix_nanos(pd.Timestamp(row["Start"])),
                ts_init=maybe_dt_to_unix_nanos(pd.Timestamp(row["Start"])),
            )

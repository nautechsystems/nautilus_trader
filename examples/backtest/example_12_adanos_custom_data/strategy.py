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

from nautilus_trader.adapters.adanos import AdanosSentimentSnapshot
from nautilus_trader.adapters.adanos import adanos_sentiment_data_type
from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import StrategyConfig
from nautilus_trader.core.data import Data
from nautilus_trader.model import CustomData
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.trading.strategy import Strategy


ADANOS_CLIENT_ID = ClientId("ADANOS")


class AdanosSentimentStrategyConfig(StrategyConfig, frozen=True):
    instrument_id: InstrumentId
    bar_type: BarType


class AdanosSentimentStrategy(Strategy):
    def __init__(self, config: AdanosSentimentStrategyConfig) -> None:
        super().__init__(config)
        self._latest_sentiment: AdanosSentimentSnapshot | None = None

    def on_start(self) -> None:
        self.subscribe_bars(self.config.bar_type)
        self.subscribe_data(
            adanos_sentiment_data_type(self.config.instrument_id),
            client_id=ADANOS_CLIENT_ID,
        )
        self.log.info(
            f"Subscribed to bars and Adanos sentiment for {self.config.instrument_id}",
            color=LogColor.YELLOW,
        )

    def on_stop(self) -> None:
        self.unsubscribe_bars(self.config.bar_type)
        self.unsubscribe_data(
            adanos_sentiment_data_type(self.config.instrument_id),
            client_id=ADANOS_CLIENT_ID,
        )

    def on_data(self, data: Data) -> None:
        snapshot = _extract_snapshot(data)

        if snapshot is not None:
            self._latest_sentiment = snapshot
            self.log.info(
                "Sentiment update "
                f"buzz={snapshot.average_buzz:.1f} "
                f"bullish={snapshot.average_bullish_pct:.1f}% "
                f"alignment={snapshot.source_alignment}",
                color=LogColor.CYAN,
            )

    def on_bar(self, bar: Bar) -> None:
        if self._latest_sentiment is None:
            self.log.info(f"{bar.bar_type}: no sentiment snapshot yet", color=LogColor.YELLOW)
            return

        snapshot = self._latest_sentiment
        if snapshot.average_buzz >= 60.0 and snapshot.average_bullish_pct >= 55.0:
            verdict = "sentiment supports long bias"
            color = LogColor.GREEN
        elif snapshot.average_buzz < 40.0 or snapshot.source_alignment == "divergent":
            verdict = "sentiment warns against conviction"
            color = LogColor.RED
        else:
            verdict = "sentiment is neutral"
            color = LogColor.BLUE

        self.log.info(
            f"{bar.bar_type} @ {bar.close} | {verdict} "
            f"(coverage={snapshot.coverage}, alignment={snapshot.source_alignment})",
            color=color,
        )


def _extract_snapshot(data: Data) -> AdanosSentimentSnapshot | None:
    if isinstance(data, AdanosSentimentSnapshot):
        return data
    if isinstance(data, CustomData) and isinstance(data.data, AdanosSentimentSnapshot):
        return data.data
    return None

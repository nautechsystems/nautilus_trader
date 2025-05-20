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

import random

from nautilus_trader.common.component import is_matching_py


def generate_topics(n: int, seed: int) -> list[str]:
    random.seed(seed)

    cat = ["data", "info", "order"]
    model = ["quotes", "trades", "orderbooks", "depths"]
    venue = ["BINANCE", "BYBIT", "OKX", "FTX", "KRAKEN"]
    instrument = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"]

    topics: list[str] = []

    for _ in range(n):
        c = random.choice(cat)  # noqa: S311
        m = random.choice(model)  # noqa: S311
        v = random.choice(venue)  # noqa: S311
        i = random.choice(instrument)  # noqa: S311
        topics.append(f"{c}.{m}.{v}.{i}")

    return topics


def test_topic_pattern_matching(benchmark) -> None:
    topics = generate_topics(1000, 42)
    pattern = "data.*.BINANCE.ETH???"

    def match_topics():
        for topic in topics:
            is_matching_py(pattern, topic)

    benchmark(match_topics)

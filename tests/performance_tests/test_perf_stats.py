# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

import numpy as np

from nautilus_trader.core.stats import fast_mean
from nautilus_trader.core.stats import fast_std


class TestFunctionPerformance:
    def test_np_mean(self, benchmark):
        benchmark.pedantic(
            target=np.mean,
            args=(np.random.default_rng(10).random(100),),
            iterations=10_000,
            rounds=1,
        )
        # ~0ms / ~9μs / 8464ns minimum of 10000 runs @ 1 iterations each run.

    def test_np_std(self, benchmark):
        benchmark.pedantic(
            target=np.std,
            args=(np.random.default_rng(10).random(100),),
            iterations=10_000,
            rounds=1,
        )
        # ~0ms / ~20μs / 19517ns minimum of 10000 runs @ 1 iterations each run.

    def test_fast_mean(self, benchmark):
        benchmark.pedantic(
            target=fast_mean,
            args=(np.random.default_rng(10).random(100),),
            iterations=100_000,
            rounds=1,
        )
        # ~0.0ms / ~0.4μs / 440ns minimum of 100,000 runs @ 1 iteration each run.

    def test_fast_std(self, benchmark):
        benchmark.pedantic(
            target=fast_std,
            args=(np.random.default_rng(10).random(100),),
            iterations=100_000,
            rounds=1,
        )
        # ~0.0ms / ~1.0μs / 968ns minimum of 100,000 runs @ 1 iteration each run.

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

import random
from collections.abc import Callable


def auto_load_retry_delay(
    attempt: int,
    *,
    base_secs: float,
    max_secs: float,
    random_fn: Callable[[], float] = random.random,
) -> float:
    """
    Return the auto-load retry sleep duration for ``attempt``.

    Computes ``base_secs * 2**attempt`` (capped at ``max_secs``) then adds
    positive jitter of up to 25% of that capped value. The jitter prevents
    many concurrent transient subscriptions from synchronising their retries
    after a venue lifecycle race.

    Parameters
    ----------
    attempt : int
        Zero-based retry attempt index.
    base_secs : float
        Base delay applied to attempt 0 before exponentiation.
    max_secs : float
        Cap applied to the exponentiated delay before jitter.
    random_fn : Callable[[], float], default random.random
        Source of jitter in ``[0.0, 1.0)``. Overridable for deterministic tests.

    Returns
    -------
    float
        Sleep duration in seconds in ``[capped_delay, capped_delay * 1.25)``.

    """
    delay = min(max_secs, base_secs * (2**attempt))
    return delay + delay * 0.25 * random_fn()

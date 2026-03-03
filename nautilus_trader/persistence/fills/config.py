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

from __future__ import annotations

from typing import Literal

from nautilus_trader.common.config import ActorConfig
from nautilus_trader.common.config import PositiveInt

ErrorPolicy = Literal["fail_fast", "log_and_drop", "buffer_until_full_then_fail"]


class ExecutionFillPersistenceActorConfig(ActorConfig, frozen=True):
    """
    Configuration for `ExecutionFillPersistenceActor` instances.
    """

    db_path: str
    topic: str = "events.fills.*"
    flush_interval_ms: PositiveInt = 250
    max_batch_size: PositiveInt = 1000
    flush_time_budget_ms: PositiveInt | None = 10
    max_queue_size: PositiveInt = 10_000
    on_error: ErrorPolicy = "buffer_until_full_then_fail"


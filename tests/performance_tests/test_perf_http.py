# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

import asyncio
import time

from nautilus_trader.core.nautilus_pyo3.network import HttpClient as RustClient


CONCURRENCY = 256
REQS = 1_000_00


def perf_pyo3_client() -> None:
    client = RustClient()
    url = "http://127.0.0.1:3000"

    start_time = time.perf_counter()

    asyncio.run(send_million_requests_pyo3(client, url))

    end_time = time.perf_counter()
    execution_time = end_time - start_time
    print(f"The execution time is: {execution_time}")


async def send_million_requests_pyo3(client: RustClient, url: str) -> None:
    for _ in range(int(REQS / CONCURRENCY)):
        reqs = [client.get(url, headers={}) for _ in range(CONCURRENCY)]
        tasks = asyncio.gather(*reqs)
        responses = await tasks
        for resp in responses:
            assert resp.status == 200


if __name__ == "__main__":
    perf_pyo3_client()
    # perf_cython_client()

#!/usr/bin/env python3
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

import asyncio

from nautilus_trader.core.nautilus_pyo3 import BybitHttpClient


async def main():
    client = BybitHttpClient()

    try:
        details = await client.get_account_details()

        print(f"{details.id=}")
        print(f"{details.note=}")
        print(f"{details.api_key=}")
        print(f"{details.read_only=}")
        print(f"{details.key_type=}")
        print(f"{details.user_id=}")
        print(f"{details.inviter_id=}")
        print(f"{details.vip_level=}")
        print(f"{details.mkt_maker_level=}")
        print(f"{details.affiliate_id=}")
        print(f"{details.is_master=}")
        print(f"{details.parent_uid=}")
        print(f"{details.uta=}")
        print(f"{details.kyc_level=}")
        print(f"{details.kyc_region=}")
        print(f"{details.deadline_day=}")
        print(f"{details.expired_at=}")
        print(f"{details.created_at=}")

    except Exception as e:
        print(f"Error: {e}")
        raise
    finally:
        client.cancel_all_requests()


if __name__ == "__main__":
    asyncio.run(main())

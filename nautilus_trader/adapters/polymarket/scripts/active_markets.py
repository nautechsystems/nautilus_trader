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

import ast

import requests


params = {
    "active": "true",
    "closed": "false",
    "archived": "false",
    "limit": 5,
}

resp = requests.get("https://gamma-api.polymarket.com/markets", params=params)  # type: ignore
data = resp.json()

for market in data:
    slug = market.get("slug", "")
    active = market.get("active", False)
    condition_id = market.get("conditionId", "N/A")
    clob_token_ids = market.get("clobTokenIds", "[]")

    if isinstance(clob_token_ids, str):
        try:
            clob_token_ids = ast.literal_eval(clob_token_ids)
        except Exception:
            clob_token_ids = []

    if not isinstance(clob_token_ids, list):
        clob_token_ids = []

    token_ids = ", ".join(clob_token_ids) if clob_token_ids else "N/A"

    print(f"Slug: {slug}")
    print(f"Active: {active}")
    print(f"Condition ID: {condition_id}")
    print(f"Token IDs: {token_ids}")
    print(f"Link: https://polymarket.com/event/{slug}\n")

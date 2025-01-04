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

from nautilus_trader.adapters.env import get_env_key


def get_api_key(is_demo: bool) -> str:
    if is_demo:
        return get_env_key("OKX_DEMO_API_KEY")
    return get_env_key("OKX_API_KEY")


def get_api_secret(is_demo: bool) -> str:
    if is_demo:
        return get_env_key("OKX_DEMO_API_SECRET")
    return get_env_key("OKX_API_SECRET")


def get_passphrase(is_demo: bool) -> str:
    if is_demo:
        return get_env_key("OKX_DEMO_PASSPHRASE")
    return get_env_key("OKX_PASSPHRASE")

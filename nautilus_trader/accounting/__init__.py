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

"""
The `accounting` subpackage defines both different account types and account management machinery.

There is also an `ExchangeRateCalculator` for calculating the exchange rate between FX and/or Crypto
pairs. The `AccountManager` is mainly used from the `Portfolio` to manage accounting operations.

The `AccountFactory` supports customized account types for specific integrations. These custom
account types can be registered with the factory and will then be instantiated when an `AccountState`
event is received for that integration.
"""

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
"""
The `data` subpackage groups components relating to the data stack and data tooling for
the platform.

The layered architecture of the data stack somewhat mirrors the
execution stack with a central engine, cache layer beneath, database layer
beneath, with alternative implementations able to be written on top.

Due to the high-performance, the core components are reusable between both
backtest and live implementations - helping to ensure consistent logic for
trading operations.

"""

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

from typing import Final

from nautilus_trader.model.identifiers import Venue


# CME Globex exchanges
CBCM: Final[Venue] = Venue.from_code("CBCM")
GLBX: Final[Venue] = Venue.from_code("GLBX")
NYUM: Final[Venue] = Venue.from_code("NYUM")
XCBT: Final[Venue] = Venue.from_code("XCBT")
XCEC: Final[Venue] = Venue.from_code("XCEC")
XCME: Final[Venue] = Venue.from_code("XCME")
XFXS: Final[Venue] = Venue.from_code("XFXS")
XNYM: Final[Venue] = Venue.from_code("XNYM")

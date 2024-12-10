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

class UUID4:
    """
    Represents a pseudo-random UUID (universally unique identifier)
    version 4 based on a 128-bit label as specified in RFC 4122.

    Parameters
    ----------
    value : str, optional
        The UUID value. If ``None`` then a value will be generated.

    Warnings
    --------
    - Panics at runtime if `value` is not ``None`` and not a valid UUID.

    References
    ----------
    https://en.wikipedia.org/wiki/Universally_unique_identifier
    """

    def __init__(self, value: str | None = None) -> None: ...
    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...
    @property
    def value(self) -> str: ...

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

from typing import Any, Optional

from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecAlgorithmId


cdef class ExecAlgorithmSpecification:
    """
    Represents the execution algorithm specification for the order.

    Parameters
    ----------
    client_order_id : ClientOrderId
        The client order ID for the order being executed.
    exec_algorithm_id : ExecAlgorithmId
        The execution algorithm ID.
    params : dict[str, Any], optional
        The execution algorithm parameters for the order (must be serializable primitives).
        If ``None`` then no parameters will be passed to any execution algorithm.
    """

    def __init__(
        self,
        ClientOrderId client_order_id not None,
        ExecAlgorithmId exec_algorithm_id not None,
        dict params: Optional[dict[str, Any]] = None,
    ) -> None:
        self.client_order_id = client_order_id
        self.exec_algorithm_id = exec_algorithm_id
        self.params = params
        self._key = frozenset(params.items())

    def __eq__(self, ExecAlgorithmSpecification other) -> bool:
        return (
            self.client_order_id == other.client_order_id
            and self.exec_algorithm_id == other.exec_algorithm_id
            and self._key == other._key
        )

    def __hash__(self) -> int:
        return hash((self.client_order_id.to_str(), self.exec_algorithm_id.to_str(), self._key))

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}"
            f"(client_order_id={self.client_order_id.to_str()}, "
            f"exec_algorithm_id={self.exec_algorithm_id.to_str()}, "
            f"params={self.params})"
        )

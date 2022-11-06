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

from typing import Any, Optional

from nautilus_trader.model.identifiers cimport ExecAlgorithmId


cdef class ExecAlgorithmSpecification:
    """
    Represents the execution algorithm specification for a single order submission.

    Parameters
    ----------
    exec_algorithm_id : ExecAlgorithmId
        The execution algorithm ID.
    params : dict[str, Any], optional
        The execution algorithm parameters for the order submission.

    """

    def __init__(
        self,
        ExecAlgorithmId exec_algorithm_id not None,
        dict params: Optional[dict[str, Any]] = None,
    ) -> None:
        self.exec_algorithm_id = exec_algorithm_id
        self.params = params
        self._key = frozenset(params.items())

    def __eq__(self, ExecAlgorithmSpecification other) -> bool:
        return (
            self.exec_algorithm_id == other.exec_algorithm_id
            and self._key == other._key
        )

    def __hash__(self) -> int:
        return hash((self.exec_algorithm_id, self._key))

    def __repr__(self) -> str:
        return f"{type(self).__name__}(exec_algorithm_id={self.exec_algorithm_id}, params={self.params})"

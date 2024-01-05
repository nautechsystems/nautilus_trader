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

import cython


@cython.auto_pickle(False)
cdef class Data:
    """
    The abstract base class for all data.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    @property
    def ts_event(self) -> int:
        """
        The UNIX timestamp (nanoseconds) when the data event occurred.

        Returns
        -------
        int

        """
        raise NotImplementedError("abstract property must be implemented")

    @property
    def ts_init(self) -> int:
        """
        The UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        raise NotImplementedError("abstract property must be implemented")

    @classmethod
    def fully_qualified_name(cls) -> str:
        """
        Return the fully qualified name for the `Data` class.

        Returns
        -------
        str

        References
        ----------
        https://www.python.org/dev/peps/pep-3155/

        """
        return cls.__module__ + ':' + cls.__qualname__

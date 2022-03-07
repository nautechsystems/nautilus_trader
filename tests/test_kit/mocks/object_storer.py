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


class ObjectStorer:
    """
    A test class which stores objects to assist with test assertions.
    """

    def __init__(self):
        self.count = 0
        self._store = []

    def get_store(self) -> list:
        """
        Return the list or stored objects.

        Returns
        -------
        list[Object]

        """
        return self._store

    def store(self, obj) -> None:
        """
        Store the given object.

        Parameters
        ----------
        obj : object
            The object to store.

        """
        self.count += 1
        self._store.append(obj)

    def store_2(self, obj1, obj2) -> None:
        """
        Store the given objects as a tuple.

        Parameters
        ----------
        obj1 : object
            The first object to store.
        obj2 : object
            The second object to store.

        """
        self.store((obj1, obj2))

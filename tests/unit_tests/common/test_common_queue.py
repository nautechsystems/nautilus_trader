# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import unittest

from nautilus_trader.common.queue import Queue


class QueueTests(unittest.TestCase):

    def test_queue_instantiation(self):
        # Arrange
        queue = Queue()

        # Act
        # Assert
        self.assertEqual(0, queue.maxsize)
        self.assertEqual(0, queue.qsize())
        self.assertTrue(queue.empty())
        self.assertFalse(queue.full())

    def test_put_nowait(self):
        # Arrange
        queue = Queue()

        # Act
        queue.put_nowait("A")

        # Assert
        self.assertEqual(1, queue.qsize())
        self.assertFalse(queue.empty())

    # TODO: WIP
    # def test_get_nowait(self):
    #     # Arrange
    #     queue = Queue()
    #     queue.put_nowait("A")
    #
    #     # Act
    #     item = queue.get_nowait()
    #
    #     # Assert
    #     self.assertEqual(0, queue.qsize())
    #     self.assertEqual("A", item)

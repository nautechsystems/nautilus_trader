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

import asyncio

import pytest

from nautilus_trader.common.queue import Queue


class TestQueue:
    def test_queue_instantiation(self):
        # Arrange
        queue = Queue()

        # Act, Assert
        assert queue.maxsize == 0
        assert queue.qsize() == 0
        assert queue.empty()
        assert not queue.full()

    def test_put_nowait(self):
        # Arrange
        queue = Queue()

        # Act
        queue.put_nowait("A")

        # Assert
        assert queue.qsize() == 1
        assert not queue.empty()

    def test_get_nowait(self):
        # Arrange
        queue = Queue()
        queue.put_nowait("A")

        # Act
        item = queue.get_nowait()

        # Assert
        assert queue.empty()
        assert item == "A"

    def test_put_nowait_multiple_items(self):
        # Arrange
        queue = Queue()

        # Act
        queue.put_nowait("A")
        queue.put_nowait("B")
        queue.put_nowait("C")
        queue.put_nowait("D")
        queue.put_nowait("E")

        # Assert
        assert queue.qsize() == 5
        assert not queue.empty()

    def test_put_to_maxlen_makes_queue_full(self):
        # Arrange
        queue = Queue(maxsize=5)

        # Act
        queue.put_nowait("A")
        queue.put_nowait("B")
        queue.put_nowait("C")
        queue.put_nowait("D")
        queue.put_nowait("E")

        # Assert
        assert queue.qsize() == 5
        assert queue.full()

    def test_put_nowait_onto_queue_at_maxsize_raises_queue_full(self):
        # Arrange
        queue = Queue(maxsize=5)

        # Act
        queue.put_nowait("A")
        queue.put_nowait("B")
        queue.put_nowait("C")
        queue.put_nowait("D")
        queue.put_nowait("E")

        # Assert
        with pytest.raises(asyncio.QueueFull):
            queue.put_nowait("F")

    def test_get_nowait_from_empty_queue_raises_queue_empty(self):
        # Arrange
        queue = Queue()

        # Act, Assert
        with pytest.raises(asyncio.QueueEmpty):
            queue.get_nowait()

    @pytest.mark.asyncio()
    async def test_await_put(self):
        # Arrange
        queue = Queue()
        await queue.put("A")

        # Act
        item = queue.get_nowait()

        # Assert
        assert queue.empty()
        assert item == "A"

    @pytest.mark.asyncio()
    async def test_await_get(self):
        # Arrange
        queue = Queue()
        queue.put_nowait("A")

        # Act
        item = await queue.get()

        # Assert
        assert queue.empty()
        assert item == "A"

    def test_peek_when_no_items_returns_none(self):
        # Arrange
        queue = Queue()

        # Act, Assert
        assert queue.peek_back() is None

    def test_peek_front_when_items_returns_expected_front_of_queue(self):
        # Arrange
        queue = Queue()
        queue.put_nowait("A")
        queue.put_nowait("B")
        queue.put_nowait("C")

        # Act, Assert
        assert queue.peek_front() == "A"

    def test_peek_index_when_items_returns_expected_front_of_queue(self):
        # Arrange
        queue = Queue()
        queue.put_nowait("A")
        queue.put_nowait("B")
        queue.put_nowait("C")

        # Act, Assert
        assert queue.peek_index(-1) == "A"
        assert queue.peek_index(1) == "B"
        assert queue.peek_index(0) == "C"

    def test_peek_back_when_items_returns_expected_front_of_queue(self):
        # Arrange
        queue = Queue()
        queue.put_nowait("A")
        queue.put_nowait("B")
        queue.put_nowait("C")

        # Act, Assert
        assert queue.peek_back() == "C"

    def test_as_list_when_no_items_returns_empty_list(self):
        # Arrange
        queue = Queue()

        # Act
        result = queue.to_list()

        # Assert
        assert result == []

    def test_as_list_when_items_returns_expected_list(self):
        # Arrange
        queue = Queue()
        queue.put_nowait("A")
        queue.put_nowait("B")
        queue.put_nowait("C")

        # Act
        result = queue.to_list()

        # Assert
        assert result == ["C", "B", "A"]
        assert queue.get_nowait() == "A"
        assert result == ["C", "B", "A"]  # <-- confirm was copy

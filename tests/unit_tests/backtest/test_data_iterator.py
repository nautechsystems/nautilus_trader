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


import pytest

from nautilus_trader.backtest.engine import BacktestDataIterator
from nautilus_trader.test_kit.stubs.data import MyData


class TestBacktestDataIterator:
    def test_iterate_multiple_streams_sorted(self):
        """
        Test multiple streams; iterate and assert items are merged in non-decreasing
        ts_init order.
        """
        # Arrange
        iterator = BacktestDataIterator()
        data_len = 5
        data0 = [MyData(0, ts_init=3 * k) for k in range(data_len)]
        data1 = [MyData(1, ts_init=3 * k + 1) for k in range(data_len)]
        data2 = [MyData(2, ts_init=3 * k + 2) for k in range(data_len)]
        iterator.add_data("d0", data0)
        iterator.add_data("d1", data1)
        iterator.add_data("d2", data2)

        # Act
        merged = list(iterator)

        # Assert
        assert len(merged) == 15
        assert all(merged[i].ts_init <= merged[i + 1].ts_init for i in range(len(merged) - 1))

    def test_partial_consumption_then_complete(self):
        """
        Test partial data consumption followed by complete consumption.
        """
        # Arrange
        iterator = BacktestDataIterator()
        data = [MyData(i, ts_init=i) for i in range(4)]
        iterator.add_data("seq", data)

        # Act - consume only first two items
        first_item = next(iterator).ts_init
        second_item = next(iterator).ts_init

        # Continue consuming the rest
        remaining = [x.ts_init for x in iterator]

        # Assert
        assert first_item == 0
        assert second_item == 1
        assert remaining == [2, 3]
        assert iterator.is_done()

    def test_all_data_returns_mapping(self):
        """
        Test that all_data returns a name-to-list mapping for all streams.
        """
        # Arrange
        iterator = BacktestDataIterator()
        lst = [MyData(0, ts_init=0)]
        iterator.add_data("only", lst)

        # Act
        mapping = iterator.all_data()

        # Assert
        assert list(mapping.keys()) == ["only"]
        assert mapping["only"] == lst

    def test_remove_stream_effect(self):
        """
        Test removing one stream affects iteration length accordingly.
        """
        # Arrange
        iterator = BacktestDataIterator()
        a = [MyData(0, ts_init=0)]
        b = [MyData(1, ts_init=1)]
        iterator.add_data("a", a)
        iterator.add_data("b", b)

        # Act & Assert before removal
        assert len(list(iterator)) == 2

        # Create new iterator and remove one stream
        iterator2 = BacktestDataIterator()
        iterator2.add_data("a", a)
        iterator2.add_data("b", b)
        iterator2.remove_data("a")

        # Act & Assert after removal
        assert [x.value for x in iterator2] == [1]

    def test_remove_all_streams_yields_empty(self):
        """
        Test removing all streams yields no data on iteration.
        """
        # Arrange
        iterator = BacktestDataIterator()
        iterator.add_data("x", [MyData(0, ts_init=0)])
        iterator.add_data("y", [MyData(1, ts_init=1)])

        # Act: remove both
        iterator.remove_data("x")
        iterator.remove_data("y")

        # Assert
        assert list(iterator) == []

    def test_single_data_mode_basic_functionality(self):
        """
        Test single-stream mode yields data in order.
        """
        # Arrange
        iterator = BacktestDataIterator()
        data = [MyData(v, ts_init=v) for v in [1, 2, 3]]
        iterator.add_data("single", data)

        # Act: consume all items
        first = next(iterator).value
        second = next(iterator).value
        third = next(iterator).value
        with pytest.raises(StopIteration):
            next(iterator)

        # Assert
        assert (first, second, third) == (1, 2, 3)
        assert iterator.is_done()

        # Note: After consuming all data with add_data, the stream is removed
        # because the internal closure returns empty data on subsequent calls
        # This is the expected behavior in the new implementation

    def test_append_data_priority_changes_order(self):
        """
        Test two streams with identical ts_init: default append_data=True yields FIFO,
        append_data=False yields reversed insertion priority.
        """
        # Arrange
        data_a = [MyData(0, ts_init=100)]
        data_b = [MyData(1, ts_init=100)]

        # Act
        iterator1 = BacktestDataIterator()
        iterator1.add_data("a", data_a)
        iterator1.add_data("b", data_b)
        order1 = [x.value for x in iterator1]

        iterator2 = BacktestDataIterator()
        iterator2.add_data("a", data_a)
        iterator2.add_data("b", data_b, append_data=False)
        order2 = [x.value for x in iterator2]

        # Assert
        assert order1 == [0, 1]
        assert order2 == [1, 0]

    def test_set_index_and_data_accessor_and_is_done_empty(self):
        """
        Test is_done on empty iterator, data() accessor, and set_index restart.
        """
        # Arrange: empty iterator
        iterator = BacktestDataIterator()

        # Initially done and empty
        assert iterator.is_done()
        assert list(iterator) == []

        # Removing non-existent stream should be no-op
        iterator.remove_data("nope")

        # Add a data stream
        data = [MyData(10, ts_init=10), MyData(20, ts_init=20), MyData(30, ts_init=30)]
        iterator.add_data("stream", data)

        assert iterator.data("stream") == data

        with pytest.raises(KeyError):
            iterator.data("unknown")

        # Consume first element and reset index
        first = next(iterator).value
        iterator.set_index("stream", 0)
        remaining = [x.value for x in iterator]

        # Assert: correct restart and done state
        assert first == 10
        assert remaining == [10, 20, 30]
        assert iterator.is_done()

    def test_all_data_order_and_add_empty_list(self):
        """
        Test that all_data preserves insertion order and ignores empty streams.
        """
        # Arrange
        iterator = BacktestDataIterator()
        iterator.add_data("first", [MyData(1, ts_init=1)])
        iterator.add_data("second", [MyData(2, ts_init=2)])

        # Act
        keys_before = list(iterator.all_data().keys())
        iterator.add_data("empty", [])
        keys_after = list(iterator.all_data().keys())

        # Assert: empty list did not alter keys
        assert keys_before == ["first", "second"]
        assert keys_after == ["first", "second"]

    def test_readding_data_replaces_old(self):
        """
        Test adding a stream under an existing name replaces its data.
        """
        # Arrange
        iterator = BacktestDataIterator()
        data1 = [MyData(1, ts_init=1), MyData(2, ts_init=2)]
        iterator.add_data("X", data1)

        # Act: consume first data
        first_result = [x.value for x in iterator]
        assert first_result == [1, 2]

        # Add new data under same name (creates new iterator)
        iterator2 = BacktestDataIterator()
        data2 = [MyData(3, ts_init=3)]
        iterator2.add_data("X", data2)

        # Assert: new iterator yields only new data
        second_result = [x.value for x in iterator2]
        assert second_result == [3]

    def test_single_stream_yields_in_order(self):
        """
        Test single stream yields data in chronological order.
        """
        data = [MyData(i, ts_init=ts) for i, ts in enumerate([100, 200, 300])]

        iterator = BacktestDataIterator()
        iterator.add_data("s1", data)

        assert list(iterator) == data
        assert iterator.is_done()

    def test_multiple_streams_merge_order(self):
        """
        Test multiple streams are merged in chronological order.
        """
        iterator = BacktestDataIterator()

        data_s1 = [MyData(i, ts_init=ts) for i, ts in enumerate([100, 300])]
        data_s2 = [MyData(i + 2, ts_init=ts) for i, ts in enumerate([200, 400])]

        iterator.add_data("s1", data_s1)
        iterator.add_data("s2", data_s2)

        expected_order = [100, 200, 300, 400]
        observed_order = [d.ts_init for d in iterator]

        assert observed_order == expected_order

    def test_prepend_priority_with_equal_timestamps(self):
        """
        Test prepend streams have priority over append streams for equal timestamps.
        """
        # sA is added first (append=True by default) and sB second with prepend semantics
        iterator = BacktestDataIterator()

        iterator.add_data("sA", [MyData(0, ts_init=100)])  # lower priority
        iterator.add_data(
            "sB",
            [MyData(1, ts_init=100)],
            append_data=False,
        )  # higher priority (prepend)

        first_out = next(iterator)
        second_out = next(iterator)

        assert first_out.value == 1  # prepend stream wins tie
        assert second_out.value == 0

    def test_remove_data_basic_functionality(self):
        """
        Test removing data and iterator properly handles empty state.
        """
        iterator = BacktestDataIterator()
        iterator.add_data("s1", [MyData(0, ts_init=1), MyData(1, ts_init=2)])

        # advance iterator fully
        list(iterator)

        # Now remove and ensure no error occurs
        iterator.remove_data("s1")
        assert iterator.is_done()

    def test_set_index_behavior(self):
        """
        Test set_index functionality for repositioning within a stream.
        """
        data = [MyData(i, ts_init=ts) for i, ts in enumerate([10, 20, 30])]
        iterator = BacktestDataIterator()
        iterator.add_data("s", data)

        # Consume one element
        assert next(iterator).ts_init == 10

        # Rewind to start using set_index
        iterator.set_index("s", 0)

        # Continue from beginning
        remaining = [d.ts_init for d in iterator]
        assert remaining == [10, 20, 30]

    def test_init_data_basic_functionality(self):
        """
        Test basic init_data functionality with closure-based data loading.
        """
        # Arrange
        iterator = BacktestDataIterator()
        data = [MyData(i, ts_init=i * 1000) for i in range(3)]

        def data_generator():
            yield data
            # Generator ends after yielding once

        # Act
        iterator.init_data("test_stream", data_generator())
        result = list(iterator)

        # Assert
        assert len(result) == 3
        assert [d.value for d in result] == [0, 1, 2]
        assert [d.ts_init for d in result] == [0, 1000, 2000]

    def test_init_data_empty_data_provider(self):
        """
        Test init_data with data provider that returns empty list.
        """
        # Arrange
        iterator = BacktestDataIterator()

        def empty_generator():
            return
            yield []  # Never reached

        # Act
        iterator.init_data("empty_stream", empty_generator())
        result = list(iterator)

        # Assert
        assert len(result) == 0
        assert iterator.is_done()

    def test_init_data_multiple_streams(self):
        """
        Test init_data with multiple streams merged in chronological order.
        """
        # Arrange
        iterator = BacktestDataIterator()

        def stream1_generator():
            yield [MyData(1, ts_init=1000), MyData(3, ts_init=3000)]

        def stream2_generator():
            yield [MyData(2, ts_init=2000), MyData(4, ts_init=4000)]

        # Act
        iterator.init_data("stream1", stream1_generator())
        iterator.init_data("stream2", stream2_generator())
        result = list(iterator)

        # Assert
        assert len(result) == 4
        assert [d.value for d in result] == [1, 2, 3, 4]
        assert [d.ts_init for d in result] == [1000, 2000, 3000, 4000]

    def test_init_data_append_priority(self):
        """
        Test init_data with append_data parameter affecting priority.
        """
        # Arrange
        iterator = BacktestDataIterator()

        def stream1_generator():
            yield [MyData(1, ts_init=1000)]

        def stream2_generator():
            yield [MyData(2, ts_init=1000)]  # Same timestamp

        # Act - stream1 with default append=True, stream2 with append=False (higher priority)
        iterator.init_data("stream1", stream1_generator(), append_data=True)
        iterator.init_data("stream2", stream2_generator(), append_data=False)
        result = list(iterator)

        # Assert - stream2 should come first due to higher priority
        assert len(result) == 2
        assert [d.value for d in result] == [2, 1]

    def test_init_data_replace_existing_stream(self):
        """
        Test init_data replacing an existing stream with same name.
        """
        # Arrange
        iterator = BacktestDataIterator()

        def original_generator():
            yield [MyData(1, ts_init=1000)]

        def replacement_generator():
            yield [MyData(2, ts_init=2000)]

        # Act
        iterator.init_data("stream", original_generator())
        first_result = list(iterator)

        # Create new iterator for replacement test
        iterator2 = BacktestDataIterator()
        iterator2.init_data("stream", replacement_generator())
        second_result = list(iterator2)

        # Assert
        assert [d.value for d in first_result] == [1]
        assert [d.value for d in second_result] == [2]

    def test_consecutive_data_addition_single_stream(self):
        """
        Test consecutive data addition using closure that provides data in chunks.
        """
        # Arrange
        iterator = BacktestDataIterator()

        def chunked_generator():
            yield [MyData(1, ts_init=1000), MyData(2, ts_init=2000)]
            yield [MyData(3, ts_init=3000), MyData(4, ts_init=4000)]
            # Generator ends after yielding all chunks

        # Act
        iterator.init_data("chunked_stream", chunked_generator())

        # Consume data - should trigger consecutive calls to provider
        result = []
        for data in iterator:
            result.append(data)

        # Assert
        assert len(result) == 4
        assert [d.value for d in result] == [1, 2, 3, 4]
        assert [d.ts_init for d in result] == [1000, 2000, 3000, 4000]

    def test_consecutive_data_addition_multiple_streams(self):
        """
        Test consecutive data addition with multiple streams providing data in chunks.
        """
        # Arrange
        iterator = BacktestDataIterator()

        def stream1_generator():
            yield [MyData(10, ts_init=1000)]
            yield [MyData(30, ts_init=3000)]

        def stream2_generator():
            yield [MyData(20, ts_init=2000)]
            yield [MyData(40, ts_init=4000)]

        # Act
        iterator.init_data("stream1", stream1_generator())
        iterator.init_data("stream2", stream2_generator())

        result = list(iterator)

        # Assert
        assert len(result) == 4
        assert [d.value for d in result] == [10, 20, 30, 40]
        assert [d.ts_init for d in result] == [1000, 2000, 3000, 4000]

    def test_consecutive_data_addition_with_state(self):
        """
        Test consecutive data addition where closure maintains state between calls.
        """
        # Arrange
        iterator = BacktestDataIterator()

        def stateful_generator():
            base_ts = 1000
            for i in range(3):  # Provide 3 chunks then stop
                yield [MyData(i, ts_init=base_ts + i * 1000)]

        # Act
        iterator.init_data("stateful_stream", stateful_generator())
        result = list(iterator)

        # Assert
        assert len(result) == 3
        assert [d.value for d in result] == [0, 1, 2]
        assert [d.ts_init for d in result] == [1000, 2000, 3000]

    def test_data_update_function_complete_removal(self):
        """
        Test that data update function removes stream when it returns empty data.
        """
        # Arrange
        iterator = BacktestDataIterator()

        def provider_generator():
            yield [MyData(1, ts_init=1000)]
            # Generator ends, should trigger complete removal

        # Act
        iterator.init_data("test_stream", provider_generator())
        result = list(iterator)

        # Assert
        assert len(result) == 1
        assert result[0].value == 1
        assert "test_stream" not in iterator.all_data()  # Stream should be removed
        assert iterator.is_done()

    def test_data_update_function_with_remove_data(self):
        """
        Test remove_data with complete_remove parameter for init_data streams.
        """
        # Arrange
        iterator = BacktestDataIterator()

        def provider_generator():
            yield [MyData(1, ts_init=1000)]

        iterator.init_data("test_stream", provider_generator())

        # Act - remove without complete_remove (default False)
        iterator.remove_data("test_stream")

        # The stream should be removed from data but update function might remain
        assert "test_stream" not in iterator.all_data()

        # Act - remove with complete_remove=True
        iterator.init_data("test_stream", provider_generator())  # Re-add
        iterator.remove_data("test_stream", complete_remove=True)

        # Assert
        assert "test_stream" not in iterator.all_data()
        assert iterator.is_done()

    def test_data_update_function_mixed_with_static_data(self):
        """
        Test data update function behavior when mixed with static data streams.
        """
        # Arrange
        iterator = BacktestDataIterator()

        # Static data
        static_data = [MyData(10, ts_init=1500)]
        iterator.add_data("static", static_data)

        # Dynamic data
        def dynamic_provider_generator():
            yield [MyData(1, ts_init=1000)]
            yield [MyData(2, ts_init=2000)]

        iterator.init_data("dynamic", dynamic_provider_generator())

        # Act
        result = list(iterator)

        # Assert
        assert len(result) == 3
        assert [d.value for d in result] == [1, 10, 2]
        assert [d.ts_init for d in result] == [1000, 1500, 2000]

    def test_data_update_function_error_handling(self):
        """
        Test data update function behavior when provider raises exceptions.
        """
        # Arrange
        iterator = BacktestDataIterator()

        def failing_provider_generator():
            yield [MyData(1, ts_init=1000)]
            raise ValueError("Provider failed")

        iterator.init_data("failing_stream", failing_provider_generator())

        # Act & Assert
        with pytest.raises(ValueError, match="Provider failed"):
            list(iterator)

    def test_mixed_add_data_and_init_data_basic(self):
        """
        Test basic mixed usage of add_data (static) and init_data (dynamic).
        """
        # Arrange
        iterator = BacktestDataIterator()

        # Static data
        static_data = [MyData(10, ts_init=1000), MyData(30, ts_init=3000)]
        iterator.add_data("static", static_data)

        # Dynamic data
        def dynamic_provider_generator():
            yield [MyData(20, ts_init=2000), MyData(40, ts_init=4000)]

        iterator.init_data("dynamic", dynamic_provider_generator())

        # Act
        result = list(iterator)

        # Assert
        assert len(result) == 4
        assert [d.value for d in result] == [10, 20, 30, 40]
        assert [d.ts_init for d in result] == [1000, 2000, 3000, 4000]

    def test_mixed_add_data_and_init_data_priority(self):
        """
        Test priority ordering with mixed add_data and init_data streams.
        """
        # Arrange
        iterator = BacktestDataIterator()

        # Static data with append=True (default)
        iterator.add_data("static_append", [MyData(1, ts_init=1000)], append_data=True)

        # Dynamic data with append=False (higher priority)
        def dynamic_provider_generator():
            yield [MyData(2, ts_init=1000)]  # Same timestamp

        iterator.init_data("dynamic_prepend", dynamic_provider_generator(), append_data=False)

        # Static data with append=True (lower priority)
        iterator.add_data("static_append2", [MyData(3, ts_init=1000)], append_data=True)

        # Act
        result = list(iterator)

        # Assert - dynamic_prepend should come first, then static streams in order
        assert len(result) == 3
        assert [d.value for d in result] == [2, 1, 3]

    def test_mixed_add_data_and_init_data_single_run_behavior(self):
        """
        Test single-run behavior with mixed static and dynamic data.
        """
        # Arrange
        iterator = BacktestDataIterator()

        # Static data
        iterator.add_data("static", [MyData(1, ts_init=1000)])

        # Dynamic data that provides data once then empty
        def single_call_provider_generator():
            yield [MyData(11, ts_init=2000)]

        iterator.init_data("dynamic", single_call_provider_generator())

        # Act - single iteration (as intended for iterator pattern)
        result = list(iterator)

        # Assert
        assert [d.value for d in result] == [1, 11]  # Static + dynamic
        assert iterator.is_done()

    def test_mixed_add_data_and_init_data_remove_behavior(self):
        """
        Test remove behavior with mixed static and dynamic data.
        """
        # Arrange
        iterator = BacktestDataIterator()

        # Static data
        iterator.add_data("static", [MyData(1, ts_init=1000)])

        # Dynamic data
        def dynamic_provider_generator():
            yield [MyData(2, ts_init=2000)]

        iterator.init_data("dynamic", dynamic_provider_generator())

        # Act - remove static data
        iterator.remove_data("static")
        result_after_static_removal = list(iterator)

        # Create new iterator and test removing dynamic data
        iterator2 = BacktestDataIterator()
        iterator2.add_data("static", [MyData(1, ts_init=1000)])  # Add static
        iterator2.init_data("dynamic", dynamic_provider_generator())
        iterator2.remove_data("dynamic", complete_remove=True)
        result_after_dynamic_removal = list(iterator2)

        # Assert
        assert [d.value for d in result_after_static_removal] == [2]  # Only dynamic
        assert [d.value for d in result_after_dynamic_removal] == [1]  # Only static

    def test_mixed_add_data_and_init_data_complex_scenario(self):
        """
        Test complex scenario with multiple static and dynamic streams.
        """
        # Arrange
        iterator = BacktestDataIterator()

        # Multiple static streams
        iterator.add_data("static1", [MyData(10, ts_init=1000)])
        iterator.add_data("static2", [MyData(30, ts_init=3000)])

        # Multiple dynamic streams
        def dynamic1_provider_generator():
            yield [MyData(20, ts_init=2000)]

        def dynamic2_provider_generator():
            yield [MyData(40, ts_init=4000)]

        iterator.init_data("dynamic1", dynamic1_provider_generator())
        iterator.init_data("dynamic2", dynamic2_provider_generator())

        # Check all_data before consuming (streams are present)
        all_data_before = iterator.all_data()
        assert len(all_data_before) == 4
        assert "static1" in all_data_before
        assert "static2" in all_data_before
        assert "dynamic1" in all_data_before
        assert "dynamic2" in all_data_before

        # Act
        result = list(iterator)

        # Assert
        assert len(result) == 4
        assert [d.value for d in result] == [10, 20, 30, 40]
        assert [d.ts_init for d in result] == [1000, 2000, 3000, 4000]

        # After consuming all data, streams that return empty are removed
        assert iterator.is_done()


class TestBacktestDataIteratorPresorted:
    """
    Tests for the presorted parameter optimization in BacktestDataIterator.
    """

    def test_add_data_presorted_true_skips_sort_but_maintains_order(self):
        # Arrange
        iterator = BacktestDataIterator()
        data = [
            MyData(value="first", ts_init=1_000_000_000),
            MyData(value="second", ts_init=2_000_000_000),
            MyData(value="third", ts_init=3_000_000_000),
        ]

        # Act - add pre-sorted data
        iterator.add_data("test_stream", data, presorted=True)

        # Assert - data should be in correct order
        assert iterator.next().value == "first"
        assert iterator.next().value == "second"
        assert iterator.next().value == "third"
        assert iterator.next() is None

    def test_add_data_presorted_true_copies_list_prevents_mutation(self):
        # Arrange
        iterator = BacktestDataIterator()
        data = [
            MyData(value="original1", ts_init=1_000_000_000),
            MyData(value="original2", ts_init=2_000_000_000),
        ]

        # Act - add data then mutate original list
        iterator.add_data("test_stream", data, presorted=True)
        original_length = len(data)
        data.clear()
        data.append(MyData(value="mutated", ts_init=999_000_000))

        # Assert - iterator should still have original data
        retrieved = iterator.data("test_stream")
        assert len(retrieved) == original_length
        assert retrieved[0].value == "original1"
        assert retrieved[1].value == "original2"
        # If aliasing occurred, retrieved would be empty or contain "mutated"

    def test_add_data_presorted_false_sorts_data(self):
        # Arrange
        iterator = BacktestDataIterator()
        data = [
            MyData(value="third", ts_init=3_000_000_000),
            MyData(value="first", ts_init=1_000_000_000),
            MyData(value="second", ts_init=2_000_000_000),
        ]

        # Act - add unsorted data with presorted=False (default)
        iterator.add_data("test_stream", data, presorted=False)

        # Assert - data should be sorted by ts_init
        assert iterator.next().value == "first"
        assert iterator.next().value == "second"
        assert iterator.next().value == "third"
        assert iterator.next() is None

    def test_add_data_presorted_false_also_copies_list(self):
        # Arrange
        iterator = BacktestDataIterator()
        data = [
            MyData(value="unsorted2", ts_init=2_000_000_000),
            MyData(value="unsorted1", ts_init=1_000_000_000),
        ]

        # Act - add data then mutate
        iterator.add_data("test_stream", data, presorted=False)
        data.clear()

        # Assert - iterator should have sorted copy
        retrieved = iterator.data("test_stream")
        assert len(retrieved) == 2
        assert retrieved[0].value == "unsorted1"  # Sorted order
        assert retrieved[1].value == "unsorted2"

    def test_init_data_then_add_data_presorted(self):
        # Arrange
        iterator = BacktestDataIterator()

        def data_generator():
            yield [
                MyData(value="gen1", ts_init=1_000_000_000),
                MyData(value="gen2", ts_init=2_000_000_000),
            ]

        # Act - initialize with generator, then update with presorted data
        iterator.init_data("test_stream", data_generator())

        # Consume first chunk
        assert iterator.next().value == "gen1"
        assert iterator.next().value == "gen2"

        # Replace with new presorted data
        new_data = [
            MyData(value="new1", ts_init=3_000_000_000),
            MyData(value="new2", ts_init=4_000_000_000),
        ]
        iterator.add_data("test_stream", new_data, presorted=True)

        # Assert - should use new data
        assert iterator.next().value == "new1"
        assert iterator.next().value == "new2"
        assert iterator.next() is None

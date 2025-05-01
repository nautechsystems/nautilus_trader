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

from nautilus_trader.backtest.engine import BacktestDataIterator
from nautilus_trader.test_kit.stubs.data import MyData


class TestBacktestDataIterator:
    def test_backtest_data_iterator(self):
        # Arrange

        data_iterator = BacktestDataIterator()

        data_len = 5
        data_0 = [MyData(0, ts_init=3 * k) for k in range(data_len)]
        data_1 = [MyData(0, ts_init=3 * k + 1) for k in range(data_len)]
        data_2 = [MyData(0, ts_init=3 * k + 2) for k in range(data_len)]

        # Act - Add data
        data_iterator.add_data("base", data_0)
        data_iterator.add_data("extra_1", data_1)
        data_iterator.add_data("extra_2", data_2)

        # Assert - Iterate through data
        data_result = list(data_iterator)
        assert len(data_result) == 15  # 5 items from each of the 3 data sources

        # Verify the data is sorted by ts_init
        for i in range(len(data_result) - 1):
            assert data_result[i].ts_init <= data_result[i + 1].ts_init

        # Act - Reset and iterate again
        data_iterator.reset()
        data_result_2 = list(data_iterator)

        # Assert - Same results after reset
        assert len(data_result_2) == 15
        assert [x.ts_init for x in data_result] == [x.ts_init for x in data_result_2]

        # Act - Test all_data method
        all_data = data_iterator.all_data()

        # Assert - Check all_data returns correct data
        assert len(all_data) == 3
        assert "base" in all_data
        assert "extra_1" in all_data
        assert "extra_2" in all_data
        assert all_data["base"] == data_0
        assert all_data["extra_1"] == data_1
        assert all_data["extra_2"] == data_2

        # Act - Test remove_data
        data_iterator.remove_data("extra_1")
        data_iterator.reset()
        data_result_3 = list(data_iterator)

        # Assert - Correct data after removal
        assert len(data_result_3) == 10  # 5 items from each of the 2 remaining data sources

        # Act - Remove all data
        data_iterator.remove_data("base")
        data_iterator.remove_data("extra_2")
        data_iterator.reset()
        data_result_4 = list(data_iterator)

        # Assert - No data left
        assert len(data_result_4) == 0

    def test_backtest_data_iterator_callback(self):
        # Arrange

        callback_data = []

        def empty_data_callback(data_name, last_ts_init):
            callback_data.append((data_name, last_ts_init))

        data_iterator = BacktestDataIterator(empty_data_callback=empty_data_callback)

        # Create data with different lengths
        data_0 = [MyData(0, ts_init=k) for k in range(3)]  # 0, 1, 2
        data_1 = [MyData(0, ts_init=k) for k in range(5)]  # 0, 1, 2, 3, 4

        # Act - Add data
        data_iterator.add_data("short", data_0)
        data_iterator.add_data("long", data_1)

        # Consume all data
        _ = list(data_iterator)

        # Assert - Callbacks were called for both data streams
        # The callback is called when we try to access data beyond what's available
        assert len(callback_data) == 2

        # Check that both data streams triggered callbacks
        data_names = [item[0] for item in callback_data]
        assert "short" in data_names
        assert "long" in data_names

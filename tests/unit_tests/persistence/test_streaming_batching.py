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

import os

import pandas as pd

from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.persistence.streaming.batching import generate_batches_rust
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from tests import TEST_DATA_DIR


class TestBatchingData:
    test_parquet_files = [
        os.path.join(TEST_DATA_DIR, "quote_tick_eurusd_2019_sim_rust.parquet"),
        os.path.join(TEST_DATA_DIR, "quote_tick_usdjpy_2019_sim_rust.parquet"),
        os.path.join(TEST_DATA_DIR, "bars_eurusd_2019_sim.parquet"),
    ]

    test_instruments = [
        TestInstrumentProvider.default_fx_ccy("EUR/USD", venue=Venue("SIM")),
        TestInstrumentProvider.default_fx_ccy("USD/JPY", venue=Venue("SIM")),
        TestInstrumentProvider.default_fx_ccy("EUR/USD", venue=Venue("SIM")),
    ]
    test_instrument_ids = [x.id for x in test_instruments]


class TestGenerateBatches(TestBatchingData):
    def test_generate_batches_returns_empty_list_before_start_timestamp_with_end_timestamp(self):
        start_timestamp = 1546389021944999936
        batch_gen = generate_batches_rust(
            files=[self.test_parquet_files[1]],
            cls=QuoteTick,
            batch_size=1000,
            start_nanos=start_timestamp,
            end_nanos=1546394394948999936,
        )
        batches = list(batch_gen)
        assert [len(x) for x in batches] == [0, 0, 0, 0, 172, 1000, 1000, 1000, 1000, 887]
        assert batches[4][0].ts_init == start_timestamp

        #################################
        batch_gen = generate_batches_rust(
            files=[self.test_parquet_files[1]],
            cls=QuoteTick,
            batch_size=1000,
            start_nanos=start_timestamp - 1,
            end_nanos=1546394394948999936,
        )
        batches = list(batch_gen)
        assert [len(x) for x in batches] == [0, 0, 0, 0, 172, 1000, 1000, 1000, 1000, 887]
        assert batches[4][0].ts_init == start_timestamp

    def test_generate_batches_returns_batches_of_expected_size(self):
        batch_gen = generate_batches_rust(
            files=[self.test_parquet_files[1]],
            cls=QuoteTick,
            batch_size=1000,
        )
        batches = list(batch_gen)
        assert all(len(x) == 1000 for x in batches)

    def test_generate_batches_returns_empty_list_before_start_timestamp(self):
        # Arrange
        parquet_data_path = self.test_parquet_files[0]
        start_timestamp = 1546383601403000064  # index 10 (1st item in batch)
        batch_gen = generate_batches_rust(
            files=[parquet_data_path],
            cls=QuoteTick,
            batch_size=10,
            start_nanos=start_timestamp,
        )

        # Act
        batch = next(batch_gen, None)

        # Assert
        assert batch == []

        #############################################
        # Arrange
        parquet_data_path = self.test_parquet_files[0]
        start_timestamp = 1546383601862999808  # index 18 (last item in batch)
        batch_gen = generate_batches_rust(
            files=[parquet_data_path],
            cls=QuoteTick,
            batch_size=10,
            start_nanos=start_timestamp,
        )
        # Act
        batch = next(batch_gen, None)

        # Assert
        assert batch == []

        ###################################################
        # Arrange
        parquet_data_path = self.test_parquet_files[0]
        start_timestamp = 1546383601352000000  # index 9
        batch_gen = generate_batches_rust(
            files=[parquet_data_path],
            cls=QuoteTick,
            batch_size=10,
            start_nanos=start_timestamp,
        )

        # Act
        batch = next(batch_gen, None)

        # Assert
        assert batch != []

    def test_generate_batches_trims_first_batch_by_start_timestamp(self):
        def create_test_batch_gen(start_timestamp):
            parquet_data_path = self.test_parquet_files[0]
            return generate_batches_rust(
                files=[parquet_data_path],
                cls=QuoteTick,
                batch_size=10,
                start_nanos=start_timestamp,
            )

        start_timestamp = 1546383605776999936
        batches = list(
            generate_batches_rust(
                files=[self.test_parquet_files[0]],
                cls=QuoteTick,
                batch_size=300,
                start_nanos=start_timestamp,
            ),
        )

        first_timestamp = batches[0][0].ts_init
        assert first_timestamp == start_timestamp

        ###############################################################
        # Timestamp, index -1, exists
        start_timestamp = 1546383601301000192  # index 8
        batch_gen = create_test_batch_gen(start_timestamp)

        # Act
        batches = list(batch_gen)

        # Assert
        first_timestamp = batches[0][0].ts_init
        assert first_timestamp == start_timestamp

        ###############################################################
        # Timestamp, index 0, exists
        start_timestamp = 1546383600078000128  # index 0
        batch_gen = create_test_batch_gen(start_timestamp)

        # Act
        batches = list(batch_gen)

        # Assert
        first_timestamp = batches[0][0].ts_init
        assert first_timestamp == start_timestamp

        ###############################################################
        # Timestamp, index 0, NOT exists
        start_timestamp = 1546383600078000128  # index 0
        batch_gen = create_test_batch_gen(start_timestamp - 1)

        # Act
        batches = list(batch_gen)

        # Assert
        first_timestamp = batches[0][0].ts_init
        assert first_timestamp == start_timestamp

        ###############################################################
        # Timestamp, index -1, NOT exists
        start_timestamp = 1546383601301000192  # index 8
        batch_gen = create_test_batch_gen(start_timestamp - 1)

        # Act
        batches = list(batch_gen)

        # Assert
        first_timestamp = batches[0][0].ts_init
        assert first_timestamp == start_timestamp
        ###############################################################
        # Arrange

        start_timestamp = 1546383600691000064
        batch_gen = create_test_batch_gen(start_timestamp)

        # Act
        batches = list(batch_gen)

        # Assert
        first_batch = batches[0]
        print(len(first_batch))
        assert len(first_batch) == 5

        first_timestamp = first_batch[0].ts_init
        assert first_timestamp == start_timestamp
        ###############################################################
        # Starts on next timestamp if start_timestamp NOT exists
        # Arrange
        start_timestamp = 1546383600078000128  # index 0
        next_timestamp = 1546383600180000000  # index 1
        batch_gen = create_test_batch_gen(start_timestamp + 1)

        # Act
        batches = list(batch_gen)

        # Assert
        first_timestamp = batches[0][0].ts_init
        assert first_timestamp == next_timestamp

    def test_generate_batches_trims_end_batch_returns_no_empty_batch(self):
        parquet_data_path = self.test_parquet_files[0]

        # Timestamp, index -1, NOT exists
        # Arrange
        end_timestamp = 1546383601914000128  # index 19
        batch_gen = generate_batches_rust(
            files=[parquet_data_path],
            cls=QuoteTick,
            batch_size=10,
            end_nanos=end_timestamp,
        )

        # Act
        batches = list(batch_gen)

        # Assert
        last_batch = batches[-1]
        assert last_batch != []

    def test_generate_batches_trims_end_batch_by_end_timestamp(self):
        def create_test_batch_gen(end_timestamp):
            parquet_data_path = self.test_parquet_files[0]
            return generate_batches_rust(
                files=[parquet_data_path],
                cls=QuoteTick,
                batch_size=10,
                end_nanos=end_timestamp,
            )

        ###############################################################
        # Timestamp, index 0
        end_timestamp = 1546383601403000064  # index 10
        batches = list(create_test_batch_gen(end_timestamp))
        last_timestamp = batches[-1][-1].ts_init
        assert last_timestamp == end_timestamp

        batches = list(create_test_batch_gen(end_timestamp + 1))
        last_timestamp = batches[-1][-1].ts_init
        assert last_timestamp == end_timestamp

        ###############################################################
        # Timestamp index -1
        end_timestamp = 1546383601914000128  # index 19

        batches = list(create_test_batch_gen(end_timestamp))
        last_timestamp = batches[-1][-1].ts_init
        assert last_timestamp == end_timestamp

        batches = list(create_test_batch_gen(end_timestamp + 1))
        last_timestamp = batches[-1][-1].ts_init
        assert last_timestamp == end_timestamp

        ###############################################################
        # Ends on prev timestamp

        end_timestamp = 1546383601301000192  # index 8
        prev_timestamp = 1546383601197999872  # index 7
        batches = list(create_test_batch_gen(end_timestamp - 1))
        last_timestamp = batches[-1][-1].ts_init
        assert last_timestamp == prev_timestamp

    def test_generate_batches_returns_valid_data_quote_tick(self):
        # Arrange
        parquet_data_path = self.test_parquet_files[0]
        batch_gen = generate_batches_rust(
            files=[parquet_data_path],
            cls=QuoteTick,
            batch_size=300,
        )

        expected = pd.read_parquet(parquet_data_path)

        # Act
        results = []
        for batch in batch_gen:
            results.extend(batch)

        # Assert
        assert len(results) == len(expected)
        assert [x.ts_init for x in results] == list(expected.ts_init)

    def test_generate_batches_returns_valid_data_trade_tick(self):
        # Arrange
        parquet_data_path = os.path.join(TEST_DATA_DIR, "trade_tick_data.parquet")
        batch_gen = generate_batches_rust(
            files=[parquet_data_path],
            cls=TradeTick,
            batch_size=300,
        )

        expected = pd.read_parquet(parquet_data_path)

        # Act
        results = []
        for batch in batch_gen:
            results.extend(batch)

        # Assert
        assert len(results) == len(expected)
        assert [x.ts_init for x in results] == list(expected.ts_init)

    def test_generate_batches_returns_has_inclusive_start_and_end(self):
        # Arrange
        parquet_data_path = self.test_parquet_files[0]

        expected = pd.read_parquet(parquet_data_path)

        batch_gen = generate_batches_rust(
            files=[parquet_data_path],
            cls=QuoteTick,
            batch_size=500,
            start_nanos=expected.iloc[0].ts_init,
            end_nanos=expected.iloc[-1].ts_init,
        )

        # Act
        results = []
        for batch in batch_gen:
            results.extend(batch)

        # Assert
        assert len(results) == len(expected)
        assert [x.ts_init for x in results] == list(expected.ts_init)

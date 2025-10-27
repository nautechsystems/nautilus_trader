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

# import pickle
#
# import pandas as pd
#
# from nautilus_trader.adapters.databento.enums import DatabentoStatisticType
# from nautilus_trader.adapters.databento.enums import DatabentoStatisticUpdateAction
# from nautilus_trader.adapters.databento.types import DatabentoImbalance
# from nautilus_trader.adapters.databento.types import DatabentoStatistics
# from nautilus_trader.model.enums import OrderSide
# from nautilus_trader.model.identifiers import InstrumentId
# from nautilus_trader.model.objects import Price
# from nautilus_trader.model.objects import Quantity
# from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
#
#
# def test_imbalance_hash_str_repr() -> None:
#     # Arrange
#     imbalance = DatabentoImbalance(
#         instrument_id=TestIdStubs.msft_xnas_id(),
#         ref_price=Price.from_str("238.94"),
#         cont_book_clr_price=Price.from_str("238.94"),
#         auct_interest_clr_price=Price.from_str("238.94"),
#         paired_qty=Quantity.from_int(242735),
#         total_imbalance_qty=Quantity.from_int(1248),
#         side=OrderSide.BUY,
#         significant_imbalance="L",
#         ts_event=pd.Timestamp("2022-09-29T13:28:00.039784528Z").value,
#         ts_init=pd.Timestamp("2022-09-29T13:28:00.039774464Z").value,
#     )
#
#     # Act, Assert
#     assert (
#         str(imbalance)
#         == "DatabentoImbalance(instrument_id=MSFT.XNAS, ref_price=238.94, cont_book_clr_price=238.94, auct_interest_clr_price=238.94, paired_qty=242735, total_imbalance_qty=1248, side=BUY, significant_imbalance=L, ts_event=1664458080039784528, ts_init=1664458080039774464)"
#     )
#     assert (
#         repr(imbalance)
#         == "DatabentoImbalance(instrument_id=MSFT.XNAS, ref_price=238.94, cont_book_clr_price=238.94, auct_interest_clr_price=238.94, paired_qty=242735, total_imbalance_qty=1248, side=BUY, significant_imbalance=L, ts_event=1664458080039784528, ts_init=1664458080039774464)"
#     )
#     assert isinstance(hash(imbalance), int)
#
#
# def test_imbalance_pickling() -> None:
#     # Arrange
#     imbalance = DatabentoImbalance(
#         instrument_id=TestIdStubs.msft_xnas_id(),
#         ref_price=Price.from_str("238.94"),
#         cont_book_clr_price=Price.from_str("238.94"),
#         auct_interest_clr_price=Price.from_str("238.94"),
#         paired_qty=Quantity.from_int(242735),
#         total_imbalance_qty=Quantity.from_int(1248),
#         side=OrderSide.BUY,
#         significant_imbalance="L",
#         ts_event=pd.Timestamp("2022-09-29T13:28:00.039784528Z").value,
#         ts_init=pd.Timestamp("2022-09-29T13:28:00.039774464Z").value,
#     )
#
#     # Act
#     pickled = pickle.dumps(imbalance)
#     unpickled = pickle.loads(pickled)  # (pickle is safe here)
#
#     # Assert
#     assert unpickled == imbalance
#     assert (
#         repr(unpickled)
#         == "DatabentoImbalance(instrument_id=MSFT.XNAS, ref_price=238.94, cont_book_clr_price=238.94, auct_interest_clr_price=238.94, paired_qty=242735, total_imbalance_qty=1248, side=BUY, significant_imbalance=L, ts_event=1664458080039784528, ts_init=1664458080039774464)"
#     )
#
#
# def test_to_dict_from_dict_round_trip() -> None:
#     # Arrange
#     imbalance = DatabentoImbalance(
#         instrument_id=TestIdStubs.msft_xnas_id(),
#         ref_price=Price.from_str("238.94"),
#         cont_book_clr_price=Price.from_str("238.94"),
#         auct_interest_clr_price=Price.from_str("238.94"),
#         paired_qty=Quantity.from_int(242735),
#         total_imbalance_qty=Quantity.from_int(1248),
#         side=OrderSide.BUY,
#         significant_imbalance="L",
#         ts_event=pd.Timestamp("2022-09-29T13:28:00.039784528Z").value,
#         ts_init=pd.Timestamp("2022-09-29T13:28:00.039774464Z").value,
#     )
#
#     # Act
#     values = imbalance.to_dict(imbalance)
#
#     # Assert
#     assert DatabentoImbalance.from_dict(values) == imbalance
#     assert values == {
#         "type": "DatabentoImbalance",
#         "instrument_id": "MSFT.XNAS",
#         "ref_price": "238.94",
#         "cont_book_clr_price": "238.94",
#         "auct_interest_clr_price": "238.94",
#         "paired_qty": "242735",
#         "total_imbalance_qty": "1248",
#         "side": "BUY",
#         "significant_imbalance": "L",
#         "ts_event": 1664458080039784528,
#         "ts_init": 1664458080039774464,
#     }
#
#
# def test_statistics_hash_str_repr() -> None:
#     # Arrange
#     statistics = DatabentoStatistics(
#         instrument_id=InstrumentId.from_str("TSLA 230901C00250000.XBOX"),
#         stat_type=DatabentoStatisticType.TRADING_SESSION_HIGH_PRICE,
#         update_action=DatabentoStatisticUpdateAction.ADDED,
#         price=Price.from_str("3.450000000"),
#         quantity=None,
#         channel_id=41,
#         stat_flags=0,
#         sequence=1278617494,
#         ts_ref=1,
#         ts_in_delta=2,
#         ts_event=pd.Timestamp("2022-09-29T13:28:00.039784528Z").value,
#         ts_init=pd.Timestamp("2022-09-29T13:28:00.039774464Z").value,
#     )
#
#     # Act, Assert
#     assert (
#         str(statistics)
#         == "DatabentoStatistics(instrument_id=TSLA 230901C00250000.XBOX, stat_type=DatabentoStatisticType.TRADING_SESSION_HIGH_PRICE, update_action=DatabentoStatisticUpdateAction.ADDED, price=3.450000000, quantity=None, channel_id=41, stat_flags=0, sequence=1278617494, ts_ref=1, ts_in_delta=2, ts_event=1664458080039784528, ts_init=1664458080039774464)"
#     )
#     assert (
#         repr(statistics)
#         == "DatabentoStatistics(instrument_id=TSLA 230901C00250000.XBOX, stat_type=DatabentoStatisticType.TRADING_SESSION_HIGH_PRICE, update_action=DatabentoStatisticUpdateAction.ADDED, price=3.450000000, quantity=None, channel_id=41, stat_flags=0, sequence=1278617494, ts_ref=1, ts_in_delta=2, ts_event=1664458080039784528, ts_init=1664458080039774464)"
#     )
#     assert isinstance(hash(statistics), int)
#
#
# def test_statistics_pickle() -> None:
#     # Arrange
#     statistics = DatabentoStatistics(
#         instrument_id=InstrumentId.from_str("TSLA 230901C00250000.XBOX"),
#         stat_type=DatabentoStatisticType.TRADING_SESSION_HIGH_PRICE,
#         update_action=DatabentoStatisticUpdateAction.ADDED,
#         price=Price.from_str("3.450000000"),
#         quantity=None,
#         channel_id=41,
#         stat_flags=0,
#         sequence=1278617494,
#         ts_ref=1,
#         ts_in_delta=2,
#         ts_event=pd.Timestamp("2022-09-29T13:28:00.039784528Z").value,
#         ts_init=pd.Timestamp("2022-09-29T13:28:00.039774464Z").value,
#     )
#
#     # Act
#     pickled = pickle.dumps(statistics)
#     unpickled = pickle.loads(pickled)  # (pickle is safe here)
#
#     # Assert
#     assert unpickled == statistics
#
#
# def test_statistics_to_dict_from_dict_round_trip() -> None:
#     # Arrange
#     statistics = DatabentoStatistics(
#         instrument_id=InstrumentId.from_str("TSLA 230901C00250000.XBOX"),
#         stat_type=DatabentoStatisticType.TRADING_SESSION_HIGH_PRICE,
#         update_action=DatabentoStatisticUpdateAction.ADDED,
#         price=Price.from_str("3.450000000"),
#         quantity=None,
#         channel_id=41,
#         stat_flags=0,
#         sequence=1278617494,
#         ts_ref=1,
#         ts_in_delta=2,
#         ts_event=pd.Timestamp("2022-09-29T13:28:00.039784528Z").value,
#         ts_init=pd.Timestamp("2022-09-29T13:28:00.039774464Z").value,
#     )
#
#     # Act
#     values = statistics.to_dict(statistics)
#
#     # Assert
#     assert DatabentoStatistics.from_dict(values) == statistics
#     assert values == {
#         "type": "DatabentoStatistics",
#         "instrument_id": "TSLA 230901C00250000.XBOX",
#         "stat_type": 5,
#         "update_action": 1,
#         "price": "3.450000000",
#         "quantity": None,
#         "channel_id": 41,
#         "stat_flags": 0,
#         "sequence": 1278617494,
#         "ts_ref": 1,
#         "ts_in_delta": 2,
#         "ts_event": 1664458080039784528,
#         "ts_init": 1664458080039774464,
#     }

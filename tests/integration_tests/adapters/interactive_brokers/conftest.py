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
# from unittest.mock import Mock
#
# import pytest
# from eventkit import Event
# from ib_insync import IB
# from ib_insync import Contract
# from ib_insync import Ticker
#
# from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestExecStubs
#
#
# class FakeIB(IB):
#     errorEvent = Event("errorEvent")
#     newOrderEvent = Event("newOrderEvent")
#     orderModifyEvent = Event("orderModifyEvent")
#     cancelOrderEvent = Event("cancelOrderEvent")
#     openOrderEvent = Event("openOrderEvent")
#     orderStatusEvent = Event("orderStatusEvent")
#     execDetailsEvent = Event("execDetailsEvent")
#
#     _tickers: list[Ticker] = []
#     reqMktData = Mock(return_value=Ticker(contract=Contract(conId=1)))
#     reqMktDepth = Mock(return_value=Ticker(contract=Contract(conId=1)))
#     placeOrder = Mock(return_value=IBTestExecStubs.trade_submitted())
#     cancelOrder = Mock()

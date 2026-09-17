# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
"""
Implement market data subscriptions and historical request hooks.
"""

from typing import override

from nautilus_trader import live
from nautilus_trader.live.clients import MarketDataClient
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick

from .constants import NOT_IMPLEMENTED


class TemplateDataClient(MarketDataClient):
    """
    Publish deterministic venue data through queued core output.
    """

    async def _connect(self) -> None:
        if self.instrument_provider is None:
            raise RuntimeError("The template data client requires an instrument provider")
        await self.instrument_provider.initialize()
        for instrument in self.instrument_provider.list_all():
            self._handle_instrument(instrument)

    async def _disconnect(self) -> None:
        pass

    async def _subscribe_quotes(self, command) -> None:
        self._handle_data(
            QuoteTick(
                instrument_id=command.instrument_id,
                bid_price=Price.from_str("1.12345"),
                ask_price=Price.from_str("1.12349"),
                bid_size=Quantity.from_str("17000"),
                ask_size=Quantity.from_str("23000"),
                ts_event=19,
                ts_init=29,
            ),
        )

    async def _unsubscribe_quotes(self, command) -> None:
        pass

    @override
    async def _subscribe(self, command: live.SubscribeCustomData) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _subscribe_instruments(self, command: live.SubscribeInstruments) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _subscribe_instrument(self, command: live.SubscribeInstrument) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _subscribe_book_deltas(self, command: live.SubscribeBookDeltas) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _subscribe_book_depth(self, command: live.SubscribeBookDepth) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _subscribe_trades(self, command: live.SubscribeTrades) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _subscribe_mark_prices(self, command: live.SubscribeMarkPrices) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _subscribe_index_prices(self, command: live.SubscribeIndexPrices) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _subscribe_funding_rates(self, command: live.SubscribeFundingRates) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _subscribe_bars(self, command: live.SubscribeBars) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _subscribe_instrument_status(self, command: live.SubscribeInstrumentStatus) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _subscribe_instrument_close(self, command: live.SubscribeInstrumentClose) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _subscribe_option_greeks(self, command: live.SubscribeOptionGreeks) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _unsubscribe(self, command: live.UnsubscribeCustomData) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _unsubscribe_instruments(self, command: live.UnsubscribeInstruments) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _unsubscribe_instrument(self, command: live.UnsubscribeInstrument) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _unsubscribe_book_deltas(self, command: live.UnsubscribeBookDeltas) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _unsubscribe_book_depth(self, command: live.UnsubscribeBookDepth) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _unsubscribe_trades(self, command: live.UnsubscribeTrades) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _unsubscribe_mark_prices(self, command: live.UnsubscribeMarkPrices) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _unsubscribe_index_prices(self, command: live.UnsubscribeIndexPrices) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _unsubscribe_funding_rates(self, command: live.UnsubscribeFundingRates) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _unsubscribe_bars(self, command: live.UnsubscribeBars) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _unsubscribe_instrument_status(
        self,
        command: live.UnsubscribeInstrumentStatus,
    ) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _unsubscribe_instrument_close(self, command: live.UnsubscribeInstrumentClose) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _unsubscribe_option_greeks(self, command: live.UnsubscribeOptionGreeks) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _request_data(self, request: live.RequestCustomData) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _request_instruments(self, request: live.RequestInstruments) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _request_instrument(self, request: live.RequestInstrument) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _request_book_snapshot(self, request: live.RequestBookSnapshot) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _request_quotes(self, request: live.RequestQuotes) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _request_trades(self, request: live.RequestTrades) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _request_funding_rates(self, request: live.RequestFundingRates) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _request_option_chain_reference_price(
        self,
        request: live.RequestOptionChainReferencePrice,
    ) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _request_bars(self, request: live.RequestBars) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _request_book_depth(self, request: live.RequestBookDepth) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

    @override
    async def _request_book_deltas(self, request: live.RequestBookDeltas) -> None:
        raise NotImplementedError(NOT_IMPLEMENTED)

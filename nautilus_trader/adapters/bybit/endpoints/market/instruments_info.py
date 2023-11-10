from typing import Union

import msgspec

from nautilus_trader.adapters.bybit.common.enums import BybitEndpointType
from nautilus_trader.adapters.bybit.common.enums import BybitInstrumentType
from nautilus_trader.adapters.bybit.endpoints.endpoint import BybitHttpEndpoint
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.schemas.market.instrument import BybitInstrumentsLinearResponse
from nautilus_trader.adapters.bybit.schemas.market.instrument import BybitInstrumentsOptionResponse
from nautilus_trader.adapters.bybit.schemas.market.instrument import BybitInstrumentsSpotResponse
from nautilus_trader.core.nautilus_pyo3.network import HttpMethod


class BybitInstrumentsInfoGetParameters(msgspec.Struct, omit_defaults=True, frozen=False):
    category: str = None
    symbol: str = None
    status: str = None


class BybitInstrumentsInfoEndpoint(BybitHttpEndpoint):
    def __init__(
        self,
        client: BybitHttpClient,
        base_endpoint: str,
        instrument_type: BybitInstrumentType,
    ):
        self.instrument_type = instrument_type
        url_path = base_endpoint + "instruments-info"
        super().__init__(
            client=client,
            endpoint_type=BybitEndpointType.MARKET,
            url_path=url_path,
        )
        self._response_decoder_instrument_linear = msgspec.json.Decoder(
            BybitInstrumentsLinearResponse,
        )
        self._response_decoder_instrument_spot = msgspec.json.Decoder(BybitInstrumentsSpotResponse)
        self._response_decoder_instrument_option = msgspec.json.Decoder(
            BybitInstrumentsOptionResponse
        )

    async def get(
        self,
        parameters: BybitInstrumentsInfoGetParameters,
    ) -> Union[
        BybitInstrumentsLinearResponse,
        BybitInstrumentsSpotResponse,
        BybitInstrumentsOptionResponse,
    ]:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, parameters)
        if self.instrument_type == BybitInstrumentType.LINEAR:
            return self._response_decoder_instrument_linear.decode(raw)
        elif self.instrument_type == BybitInstrumentType.SPOT:
            return self._response_decoder_instrument_spot.decode(raw)
        elif self.instrument_type == BybitInstrumentType.OPTION:
            return self._response_decoder_instrument_option.decode(raw)
        else:
            raise ValueError("Invalid account type")

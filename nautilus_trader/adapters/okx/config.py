from nautilus_trader.adapters.okx.common.enums import OKXContractType
from nautilus_trader.adapters.okx.common.enums import OKXInstrumentType
from nautilus_trader.adapters.okx.common.enums import OKXMarginMode
from nautilus_trader.adapters.okx.common.enums import OKXWsBaseUrlType
from nautilus_trader.adapters.okx.common.urls import get_http_base_url
from nautilus_trader.adapters.okx.common.urls import get_ws_base_url
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig


class OKXDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``OKXDataClient`` instances.

    api_key : str, [default=None]
        The OKX API public key.
        If ``None`` then will source the `OKX_API_KEY` environment variable.
    api_secret : str, [default=None]
        The OKX API secret key.
        If ``None`` then will source the `OKX_API_SECRET` environment variable.
    passphrase : str, [default=None]
        The passphrase used when creating the OKX API keys.
        If ``None`` then will source the `OKX_PASSPHRASE` environment variable.
    instrument_types : tuple[OKXInstrumentType], optional
        The OKX instrument types of instruments to load. The default is `[OKXInstrumentType.SWAP]`.
        If None, all instrument types are loaded (subject to contract types and their compatibility
        with instrument types).
    contract_types : tuple[OKXInstrumentType], optional
        The OKX contract types of instruments to load. The default is `[OKXInstrumentType.LINEAR]`.
        If None, all contract types are loaded (subject to instrument types and their compatibility
        with contract types).
    base_url_http : str, optional
        The base url to OKX's http api.
        If ``None`` then will source the `OKX_BASE_URL_HTTP` environment variable.
    base_url_public_ws : str, optional
        The base url to OKX's public websocket api.
        If ``None`` then will source the url from `get_ws_base_url(OKXWsBaseUrlType.PUBLIC)`.
    base_url_private_ws : str, optional
        The base url to OKX's private websocket api.
        If ``None`` then will source the url from `get_ws_base_url(OKXWsBaseUrlType.PRIVATE)`.
    base_url_business_ws : str, optional
        The base url to OKX's business websocket api.
        If ``None`` then will source the url from `get_ws_base_url(OKXWsBaseUrlType.BUSINESS)`.
    demo_base_url_public_ws : str, optional
        The base url to OKX's demo public websocket api.
        If ``None`` then will source the url from `get_ws_base_url(OKXWsBaseUrlType.PUBLIC, True)`.
    demo_base_url_private_ws : str, optional
        The base url to OKX's demo private websocket api.
        If ``None`` then will source the url from `get_ws_base_url(OKXWsBaseUrlType.PRIVATE, True)`.
    demo_base_url_business_ws : str, optional
        The base url to OKX's demo business websocket api.
        If ``None`` then will source the url from
        `get_ws_base_url(OKXWsBaseUrlType.BUSINESS, True)`.
    is_demo : bool, default False
        If the client is connecting to the OKX demo API.

    """

    api_key: str | None = None
    api_secret: str | None = None
    passphrase: str | None = None
    instrument_types: tuple[OKXInstrumentType] | None = (OKXInstrumentType.SWAP,)
    contract_types: tuple[OKXContractType] | None = (OKXContractType.LINEAR,)
    base_url_http: str | None = get_http_base_url()
    base_url_public_ws: str | None = get_ws_base_url(OKXWsBaseUrlType.PUBLIC, is_demo=False)
    base_url_private_ws: str | None = get_ws_base_url(OKXWsBaseUrlType.PRIVATE, is_demo=False)
    base_url_business_ws: str | None = get_ws_base_url(OKXWsBaseUrlType.BUSINESS, is_demo=False)
    demo_base_url_public_ws: str | None = get_ws_base_url(OKXWsBaseUrlType.PUBLIC, is_demo=True)
    demo_base_url_private_ws: str | None = get_ws_base_url(OKXWsBaseUrlType.PRIVATE, is_demo=True)
    demo_base_url_business_ws: str | None = get_ws_base_url(OKXWsBaseUrlType.BUSINESS, is_demo=True)
    is_demo: bool = False

    def get_applicable_ws_base_url(self, ws_base_url_type: OKXWsBaseUrlType) -> str | None:
        if self.is_demo:
            match ws_base_url_type:
                case OKXWsBaseUrlType.PUBLIC:
                    return self.demo_base_url_public_ws
                case OKXWsBaseUrlType.PRIVATE:
                    return self.demo_base_url_private_ws
                case OKXWsBaseUrlType.BUSINESS:
                    return self.demo_base_url_business_ws

        match ws_base_url_type:
            case OKXWsBaseUrlType.PUBLIC:
                return self.base_url_public_ws
            case OKXWsBaseUrlType.PRIVATE:
                return self.base_url_private_ws
            case OKXWsBaseUrlType.BUSINESS:
                return self.base_url_business_ws


class OKXExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for ``OKXExecutionClient`` instances.

    api_key : str, [default=None]
        The OKX API public key.
        If ``None`` then will source the `OKX_API_KEY` environment variable.
    api_secret : str, [default=None]
        The OKX API secret key.
        If ``None`` then will source the `OKX_API_SECRET` environment variable.
    passphrase : str, [default=None]
        The passphrase used when creating the OKX API keys.
        If ``None`` then will source the `OKX_PASSPHRASE` environment variable.
    instrument_types : tuple[OKXInstrumentType], optional
        The OKX instrument types of instruments to load. The default is `[OKXInstrumentType.SWAP]`.
        If None, all instrument types are loaded (subject to contract types and their compatibility
        with instrument types).
    contract_types : tuple[OKXInstrumentType], optional
        The OKX contract types of instruments to load. The default is `[OKXInstrumentType.LINEAR]`.
        If None, all contract types are loaded (subject to instrument types and their compatibility
        with contract types).
    base_url_http : str, optional
        The base url to OKX's http api.
        If ``None`` then will source the `OKX_BASE_URL_HTTP` environment variable.
    base_url_public_ws : str, optional
        The base url to OKX's public websocket api.
        If ``None`` then will source the url from `get_ws_base_url(OKXWsBaseUrlType.PUBLIC)`.
    base_url_private_ws : str, optional
        The base url to OKX's private websocket api.
        If ``None`` then will source the url from `get_ws_base_url(OKXWsBaseUrlType.PRIVATE)`.
    base_url_business_ws : str, optional
        The base url to OKX's business websocket api.
        If ``None`` then will source the url from `get_ws_base_url(OKXWsBaseUrlType.BUSINESS)`.
    demo_base_url_public_ws : str, optional
        The base url to OKX's demo public websocket api.
        If ``None`` then will source the url from `get_ws_base_url(OKXWsBaseUrlType.PUBLIC, True)`.
    demo_base_url_private_ws : str, optional
        The base url to OKX's demo private websocket api.
        If ``None`` then will source the url from `get_ws_base_url(OKXWsBaseUrlType.PRIVATE, True)`.
    demo_base_url_business_ws : str, optional
        The base url to OKX's demo business websocket api.
        If ``None`` then will source the url from
        `get_ws_base_url(OKXWsBaseUrlType.BUSINESS, True)`.
    margin_mode : OKXMarginMode, [default=OKXMarginMode.CROSS]
        The intended OKX account margin mode (referred to as mgnMode by OKX's docs).
    is_demo : bool, default False
        If the client is connecting to the OKX demo API.

    """

    api_key: str | None = None
    api_secret: str | None = None
    passphrase: str | None = None
    instrument_types: tuple[OKXInstrumentType] | None = (OKXInstrumentType.SWAP,)
    contract_types: tuple[OKXContractType] | None = (OKXContractType.LINEAR,)
    base_url_http: str | None = get_http_base_url()
    base_url_public_ws: str | None = get_ws_base_url(OKXWsBaseUrlType.PUBLIC, is_demo=False)
    base_url_private_ws: str | None = get_ws_base_url(OKXWsBaseUrlType.PRIVATE, is_demo=False)
    base_url_business_ws: str | None = get_ws_base_url(OKXWsBaseUrlType.BUSINESS, is_demo=False)
    demo_base_url_public_ws: str | None = get_ws_base_url(OKXWsBaseUrlType.PUBLIC, is_demo=True)
    demo_base_url_private_ws: str | None = get_ws_base_url(OKXWsBaseUrlType.PRIVATE, is_demo=True)
    demo_base_url_business_ws: str | None = get_ws_base_url(OKXWsBaseUrlType.BUSINESS, is_demo=True)
    margin_mode: OKXMarginMode = OKXMarginMode.CROSS
    is_demo: bool = False
    # use_reduce_only: bool = True  # TODO: check if applicable -> taken from Bybit

    def get_applicable_ws_base_url(self, ws_base_url_type: OKXWsBaseUrlType) -> str | None:
        if self.is_demo:
            match ws_base_url_type:
                case OKXWsBaseUrlType.PUBLIC:
                    return self.demo_base_url_public_ws
                case OKXWsBaseUrlType.PRIVATE:
                    return self.demo_base_url_private_ws
                case OKXWsBaseUrlType.BUSINESS:
                    return self.demo_base_url_business_ws

        match ws_base_url_type:
            case OKXWsBaseUrlType.PUBLIC:
                return self.base_url_public_ws
            case OKXWsBaseUrlType.PRIVATE:
                return self.base_url_private_ws
            case OKXWsBaseUrlType.BUSINESS:
                return self.base_url_business_ws

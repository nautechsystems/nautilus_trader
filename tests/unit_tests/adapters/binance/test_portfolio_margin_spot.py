from decimal import Decimal
from types import SimpleNamespace

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceEnvironment
from nautilus_trader.adapters.binance.common.enums import BinancePrivateApiFamily
from nautilus_trader.adapters.binance.common.urls import get_private_http_base_url
from nautilus_trader.adapters.binance.common.urls import get_user_stream_base_url
from nautilus_trader.adapters.binance.config import BinanceExecClientConfig
from nautilus_trader.adapters.binance.http.account import BinanceAccountHttpAPI
from nautilus_trader.adapters.binance.http.user import BinanceUserDataHttpAPI
from nautilus_trader.adapters.binance.spot.http.account import BinanceSpotAccountHttpAPI
from nautilus_trader.adapters.binance.spot.schemas.account import BinancePortfolioMarginBalanceInfo


def test_exec_client_config_accepts_portfolio_margin_account_type() -> None:
    config = BinanceExecClientConfig(account_type="PORTFOLIO_MARGIN")

    assert config.account_type == BinanceAccountType.PORTFOLIO_MARGIN


def test_private_http_base_url_routes_portfolio_margin_spot_to_papi() -> None:
    url = get_private_http_base_url(
        BinanceAccountType.PORTFOLIO_MARGIN,
        private_api_family=BinancePrivateApiFamily.AUTO,
        environment=BinanceEnvironment.LIVE,
        is_us=False,
    )

    assert url == "https://papi.binance.com"


def test_user_stream_base_url_routes_portfolio_margin_spot_to_pm_stream() -> None:
    url = get_user_stream_base_url(
        account_type=BinanceAccountType.PORTFOLIO_MARGIN,
        private_api_family=BinancePrivateApiFamily.AUTO,
        environment=BinanceEnvironment.LIVE,
        is_us=False,
    )

    assert url == "wss://fstream.binance.com/pm"


def test_portfolio_margin_user_data_api_uses_papi_listen_key() -> None:
    http_user = BinanceUserDataHttpAPI(
        client=SimpleNamespace(send_request=None, sign_request=None),
        account_type=BinanceAccountType.PORTFOLIO_MARGIN,
    )

    assert http_user._endpoint_listentoken is None
    assert http_user._endpoint_listenkey is not None
    assert http_user._endpoint_listenkey.url_path == "/papi/v1/listenKey"


def test_portfolio_margin_account_http_api_routes_orders_and_trades_to_papi_margin() -> None:
    api = BinanceAccountHttpAPI(
        client=SimpleNamespace(send_request=None, sign_request=None),
        clock=SimpleNamespace(timestamp_ms=lambda: 1_700_000_000_000),
        account_type=BinanceAccountType.PORTFOLIO_MARGIN,
    )

    assert api.base_endpoint == "/papi/v1/margin/"
    assert api._endpoint_order.url_path == "/papi/v1/margin/order"
    assert api._endpoint_open_orders.url_path == "/papi/v1/margin/openOrders"
    assert api._endpoint_user_trades.url_path == "/papi/v1/margin/myTrades"


def test_portfolio_margin_spot_account_api_uses_papi_balance_snapshot() -> None:
    api = BinanceSpotAccountHttpAPI(
        client=SimpleNamespace(send_request=None, sign_request=None),
        clock=SimpleNamespace(timestamp_ms=lambda: 1_700_000_000_000),
        account_type=BinanceAccountType.PORTFOLIO_MARGIN,
    )

    assert api._endpoint_spot_account.url_path == "/papi/v1/balance"


def test_portfolio_margin_balance_net_exposure_accounts_for_borrow_and_interest() -> None:
    balance = BinancePortfolioMarginBalanceInfo(
        asset="PLUME",
        totalWalletBalance="0",
        crossMarginAsset="0",
        crossMarginBorrowed="30721.57152347",
        crossMarginFree="0",
        crossMarginInterest="1.25000000",
        crossMarginLocked="0",
        updateTime=1,
    )

    parsed = balance.parse_to_account_balance()

    assert parsed.total.as_decimal() == Decimal("-30722.82152347")
    assert parsed.free.as_decimal() == Decimal("-30722.82152347")
    assert parsed.locked.as_decimal() == Decimal("0")

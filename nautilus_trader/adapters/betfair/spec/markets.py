from typing import Optional, Union

import msgspec


class NavigationMarket(msgspec.Struct):
    """NavigationMarket"""

    event_type_name: str
    event_type_id: str
    event_name: Optional[str] = None
    event_id: Optional[str] = None
    event_countryCode: Optional[str] = None
    market_name: str
    market_id: str
    market_exchangeId: str
    market_marketType: str
    market_marketStartTime: str
    market_numberOfWinners: Union[str, int]


class MarketDefinition(msgspec.Struct):
    """MarketDefinition"""

    pass

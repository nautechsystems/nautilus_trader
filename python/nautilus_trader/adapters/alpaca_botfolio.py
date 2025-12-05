"""
Lightweight Alpaca adapter scaffold for Bot-folio Nautilus workers.
This is a placeholder for live execution; methods must be implemented before production use.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Dict, List, Optional


@dataclass
class AlpacaAuth:
    api_key: str
    api_secret: str
    paper: bool = True
    base_url: Optional[str] = None


@dataclass
class RiskLimits:
    max_orders_per_minute: int
    max_notional_usd: Optional[float] = None
    allowed_symbols: Optional[List[str]] = None


class AlpacaBotfolioAdapter:
    """
    Placeholder adapter; wire to Alpaca REST/WS and emit Nautilus events.
    """

    def __init__(self, auth: AlpacaAuth, risk: RiskLimits) -> None:
        self.auth = auth
        self.risk = risk

    # --- Lifecycle -----------------------------------------------------
    async def start(self) -> None:
        raise NotImplementedError("start() not implemented")

    async def stop(self) -> None:
        raise NotImplementedError("stop() not implemented")

    # --- Order flow ----------------------------------------------------
    async def submit_order(
        self,
        symbol: str,
        side: str,
        quantity: float,
        order_type: str,
        tif: Optional[str] = None,
        limit_price: Optional[float] = None,
        stop_price: Optional[float] = None,
        client_order_id: Optional[str] = None,
    ) -> Dict:
        """
        Enforce risk limits then forward to Alpaca.
        """
        raise NotImplementedError("submit_order() not implemented")

    async def cancel_order(self, order_id: str) -> Dict:
        raise NotImplementedError("cancel_order() not implemented")

    # --- State / snapshots ---------------------------------------------
    async def fetch_positions(self) -> List[Dict]:
        raise NotImplementedError("fetch_positions() not implemented")

    async def fetch_cash(self) -> Dict:
        raise NotImplementedError("fetch_cash() not implemented")

    async def poll_health(self) -> Dict:
        raise NotImplementedError("poll_health() not implemented")



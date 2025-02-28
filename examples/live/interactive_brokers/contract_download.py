import asyncio

from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.historical import HistoricInteractiveBrokersClient


async def main() -> None:
    host: str = "localhost"
    port: int = 7497

    client = HistoricInteractiveBrokersClient(host=host, port=port, log_level="DEBUG")
    await client.connect()
    await asyncio.sleep(1)

    nse_nifty_fut_contract = IBContract(
        secType="FUT",
        exchange="NSE",
        symbol="NIFTY50",
        lastTradeDateOrContractMonth="20250327",
    )
    ce_contract = IBContract(
        secType="OPT",
        exchange="NSE",
        symbol="NIFTY50",
        lastTradeDateOrContractMonth="20250227",
        strike=25000,
        right="C",
        includeExpired=True,
    )
    pe_contract = IBContract(
        secType="OPT",
        exchange="NSE",
        symbol="NIFTY50",
        lastTradeDateOrContractMonth="20250227",
        strike=25000,
        right="P",
        includeExpired=True,
    )
    contracts = [nse_nifty_fut_contract, ce_contract, pe_contract]

    instruments = await client.request_instruments(
        contracts=contracts,
    )
    print(instruments)


if __name__ == "__main__":
    asyncio.run(main())

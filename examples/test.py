import logging
import os
import shutil
import sys
from decimal import Decimal

import pandas as pd

from nautilus_trader.backtest.config import BacktestDataConfig
from nautilus_trader.backtest.data.wranglers import TradeTickDataWrangler
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import CurrencyType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.currency import CurrencySpot
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.batching import batch_files
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.persistence.external.core import process_files
from nautilus_trader.persistence.external.core import write_objects
from nautilus_trader.persistence.external.readers import CSVReader


root = logging.getLogger()
root.setLevel(logging.INFO)
handler = logging.StreamHandler(sys.stdout)
handler.setLevel(logging.DEBUG)
formatter = logging.Formatter("%(asctime)s - %(name)s - %(levelname)s - %(message)s")
handler.setFormatter(formatter)
root.addHandler(handler)

KAVA = Currency("KAVA", precision=6, iso4217=0, name="KAVA", currency_type=CurrencyType.CRYPTO)
Currency.register(KAVA)


files = {
    "KAVA": "/home/brad/Downloads/KAVAUSDT.csv.gz",
}
CATALOG_PATH = "/home/brad/projects/nautilus_trader/examples/catalo"
catalog = DataCatalog(CATALOG_PATH)

instruments = {}
for curr in files:
    instruments[curr] = CurrencySpot(
        instrument_id=InstrumentId(
            symbol=Symbol(f"{curr}/USDT"),
            venue=Venue("BINANCE"),
        ),
        native_symbol=Symbol(f"{curr}USDT"),
        base_currency=Currency.from_str(curr),
        quote_currency=USDT,
        price_precision=8,
        size_precision=8,
        price_increment=Price(1e-08, precision=8),
        size_increment=Quantity(1e-08, precision=8),
        lot_size=None,
        max_quantity=Quantity(1e10, precision=8),
        min_quantity=Quantity(1e-08, precision=8),
        max_notional=None,
        min_notional=Money(1e-08, USDT),
        max_price=Price(1e10, precision=8),
        min_price=Price(1e-08, precision=8),
        margin_init=Decimal("1.00"),
        margin_maint=Decimal("0.35"),
        maker_fee=Decimal("0.0001"),
        taker_fee=Decimal("0.0001"),
        ts_event=0,
        ts_init=0,
    )


# Clear if it already exists, then create fresh
if os.path.exists(CATALOG_PATH):
    shutil.rmtree(CATALOG_PATH)
os.mkdir(CATALOG_PATH)


write_objects(catalog, list(instruments.values()))


def parser(data, instrument):
    if data is None:
        return
    print(f"{repr(instrument.id)} data chunk size {len(data)}")
    # print(data.head())
    print(
        f"{pd.Timestamp(data['timestamp'].iloc[0], unit='ms')} - {pd.Timestamp(data['timestamp'].iloc[-1], unit='ms')}",
        (data["timestamp"].diff().fillna(0) >= 0).all(),
    )
    data["side"] = data["side"].astype(int).apply(lambda x: "BUY" if x == 1 else "SELL")
    assert (data["timestamp"].diff().fillna(0) >= 0).all()
    data.loc[:, "timestamp"] = pd.to_datetime(data["timestamp"], unit="ms")
    wrangler = TradeTickDataWrangler(instrument)
    ticks = wrangler.process(data.set_index("timestamp"))
    assert (pd.Series([t.ts_init for t in ticks]).diff().fillna(0) >= 0).all()
    yield from ticks


for curr in files:
    process_files(
        glob_path=files[curr],
        reader=CSVReader(
            block_parser=lambda x: parser(x, instrument=instruments[curr]),
            header=["trade_id", "price", "quantity", "timestamp", "side"],
            chunked=True,
            as_dataframe=True,
            separator="|",
        ),
        catalog=catalog,
    )


# # Read data from the catalog and print out min and max date in each batch
# As can be seen the min and max date in each batch does not follow a chronological order


start = dt_to_unix_nanos(pd.Timestamp("2019-01-01", tz="UTC"))
end = dt_to_unix_nanos(pd.Timestamp("2022-10-30", tz="UTC"))
data_config = [
    BacktestDataConfig(  # type: ignore
        catalog_path=CATALOG_PATH,
        data_cls_path="nautilus_trader.model.data.tick.TradeTick",
        instrument_id=instrument.id.value,
        start_time=start,
        end_time=end,
    )
    for instrument in catalog.instruments(as_nautilus=True)
]
min_date = pd.Timestamp(0)
max_date = pd.Timestamp(0)

for f in batch_files(
    catalog,
    data_config,
):
    dates = [pd.Timestamp(x) for x in f]
    print(f"min date : {min(dates)}, max date: {max(dates)}")
    assert min(dates) > min_date and max(dates) > max_date
    assert max(dates) > min(dates)

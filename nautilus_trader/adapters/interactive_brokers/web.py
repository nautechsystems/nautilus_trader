import enum
from collections import namedtuple

import requests  # type: ignore
from lxml.html import fromstring  # noqa: S410


class ProductClass(enum.Enum):
    """
    Interactive Brokers Web ProductClass
    """

    ETFS = "ETF"
    INDICES = "IND"
    STOCKS = "STK"
    OPTIONS = "OPTGRP"
    WARRANTS = "WNT"


class Exchange(enum.Enum):
    """
    Interactive Brokers Exchange
    """

    AEB = "aeb"
    ALPHA = "alpha"
    AMEX = "amex"
    ARCA = "arca"
    ARCAEDGE = "arcaedge"
    ASX = "asx"
    BATECH = "batech"
    BATEDE = "batede"
    BATEEN = "bateen"
    BATEEN_BE = "bateen-be"
    BATEEN_FR = "bateen-fr"
    BATEES = "batees"
    BATEUK = "bateuk"
    BATS = "bats"
    BEX = "bex"
    BM = "bm"
    BUX = "bux"
    BVL = "bvl"
    BVME = "bvme"
    BYX = "byx"
    CHIX_CA = "chix_ca"
    CHIXAU = "chixau"
    CHIXCH = "chixch"
    CHIXDE = "chixde"
    CHIXEN = "chixen"
    CHIXEN_BE = "chixen-be"
    CHIXEN_FR = "chixen-fr"
    CHIXES = "chixes"
    CHIXJ = "chixj"
    CHIXUK = "chixuk"
    CHX = "chx"
    DRCTEDGE = "drctedge"
    DXEDE = "dxede"
    EBS = "ebs"
    EDGEA = "edgea"
    ENEXT_BE = "enext.be"
    FWB = "fwb"
    GETTEX = "gettex"
    IBIS = "ibis"
    IEX = "iex"
    ISLAND = "island"
    JPNNEXT = "jpnnext"
    LSE = "lse"
    LSEIOB1 = "lseiob1"
    LTSE = "ltse"
    MEMX = "memx"
    MEXI = "mexi"
    MOEX = "moex"
    N_RIGA = "n.riga"
    N_TALLINN = "n.tallinn"
    N_VILNIUS = "n.vilnius"
    NASDAQ = "nasdaq"
    NITE = "nite"
    NSE = "nse"
    NYSE = "nyse"
    NYSENAT = "nysenat"
    OMEGA = "omega"
    OMXNO = "omxno"
    PEARL = "pearl"
    PINK = "pink"
    PSX = "psx"
    SBF = "sbf"
    SEHK = "sehk"
    SEHKNTL = "sehkntl"
    SEHKSTAR = "sehkstar"
    SEHKSZSE = "sehkszse"
    SFB = "sfb"
    SGX = "sgx"
    SWB = "swb"
    TASE = "tase"
    TGATE = "tgate"
    TRQX_BE = "trqx-be"
    TRQX_FR = "trqx-fr"
    TRQXCH = "trqxch"
    TRQXDE = "trqxde"
    TRQXEN = "trqxen"
    TSE = "tse"
    TSEJ = "tsej"
    VENTURE = "venture"
    VIRTX = "virtx"
    VSE = "vse"
    WSE = "wse"


class Product(namedtuple("Product", "ib_symbol, description, native_symbol, currency")):
    """
    Interactive Brokers Web Product
    """

    pass


def _parse_products(table):
    for row in table.xpath(".//tr")[1:]:
        ib_symbol, desc, symbol, currency = list(
            filter(None, map(str.strip, row.xpath(".//text()")))
        )
        yield Product(
            ib_symbol=ib_symbol,
            description=desc,
            native_symbol=symbol,
            currency=currency,
        )


def load_product_list(
    exchange: Exchange, product_class: ProductClass, limit: int = 500, debug: bool = False
):
    """
    Load all instruments for a given `exchange` and `product_class` via the Interactive Brokers web interface

    >>> products = load_product_list(exchange=Exchange.NYSE, product_class=ProductClass.STOCKS)

    """
    url = "https://www.interactivebrokers.com/en/index.php"
    params = {
        "f": "2222",
        "exch": exchange.value,
        "showcategories": product_class.value,
        "limit": str(limit),
    }
    page = 0
    while True:
        page += 1
        params.update({"page": str(page)})
        if debug:
            print(f"Requesting instruments using {params=}")
        response = requests.get(url, params=params)
        tree = fromstring(response.content)
        tables = tree.xpath('//table[@class="table table-striped table-bordered"]')
        if not tables:
            break
        try:
            symbol_table = tables[2]
        except IndexError:
            break
        products = list(_parse_products(symbol_table))
        if not products:
            break
        print(f"Found {len(products)} products for {page=}")
        yield from products

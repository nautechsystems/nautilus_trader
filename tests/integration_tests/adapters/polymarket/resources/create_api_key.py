import os

from py_clob_client.client import ClobClient
from py_clob_client.constants import POLYGON


def create_polymarket_api_key():
    host = "https://clob.polymarket.com"
    key = os.environ["POLYMARKET_PK"]
    chain_id = POLYGON
    client = ClobClient(host, key=key, chain_id=chain_id)

    print(client.create_api_key())

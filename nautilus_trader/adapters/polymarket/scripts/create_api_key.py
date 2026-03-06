#!/usr/bin/env python3

import os

from py_clob_client.client import ClobClient
from py_clob_client.constants import POLYGON


client = ClobClient(
    "https://clob.polymarket.com",
    chain_id=POLYGON,
    signature_type=0,
    key=os.environ["POLYMARKET_PK"],
    funder=os.environ["POLYMARKET_FUNDER"],
)

response = client.create_or_derive_api_creds()
print(response)

import json
import os

from nautilus_trader.adapters.blockchain.utils.encoders import BlockchainDataEncoder


def save_blockchain_data_to_file(filepath: str, obj: dict, force_create=False):
    item_json = json.dumps(obj, indent=4, cls=BlockchainDataEncoder)
    if not force_create and os.path.isfile(filepath):
        return
    with open(filepath, "w", encoding="utf-8") as f:
        f.write(item_json)


def get_mock(response):
    def mock(*args, **kwargs):
        return response

    return mock

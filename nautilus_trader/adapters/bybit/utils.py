import json

import msgspec
import time
import os.path

from nautilus_trader.adapters.bybit.common.enums import BybitInstrumentType


def msgspec_bybit_item_save(filename, obj):
    item = msgspec.to_builtins(obj)
    timestamp = round(time.time()*1000)
    item_json = json.dumps(dict(retCode=0,retMsg="success",time=timestamp, result=item), indent=4)
    # check if the file already exists, if exists, do not overwrite
    if os.path.isfile(filename):
       return
    with open(filename, "w", encoding="utf-8") as f:
        f.write(item_json)


def get_category_from_instrument_type(instrument_type: BybitInstrumentType) -> str:
    if instrument_type == BybitInstrumentType.SPOT:
        return "spot"
    elif instrument_type == BybitInstrumentType.LINEAR:
        return "linear"
    elif instrument_type == BybitInstrumentType.INVERSE:
        return "inverse"
    elif instrument_type == BybitInstrumentType.OPTION:
        return "option"
    else:
        raise ValueError(f"Unknown account type: {instrument_type}")

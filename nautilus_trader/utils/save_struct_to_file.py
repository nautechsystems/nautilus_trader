import json
import os

import msgspec


def save_struct_to_file(filepath, obj, force_create=False):
    item = msgspec.to_builtins(obj)
    item_json = json.dumps(item, indent=4)
    # check if the file already exists, if exists, do not overwrite
    if not force_create and os.path.isfile(filepath):
        return
    with open(filepath, "w", encoding="utf-8") as f:
        f.write(item_json)

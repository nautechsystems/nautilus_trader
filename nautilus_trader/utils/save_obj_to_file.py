import json

import msgspec


def save_obj_to_file(filename, obj):
    obj_json = json.dumps(msgspec.to_builtins(obj), indent=4)
    with open(filename, "w", encoding="utf-8") as f:
        f.write(obj_json)

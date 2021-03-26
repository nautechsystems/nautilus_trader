import hashlib
from typing import Dict

import orjson


def flatten_tree(y: Dict, **filters):
    """
    Flatten a nested dict into a list of dicts with each nested level combined into a singe dict

    :param y: Dict
    :param filters: Filter keys
    :return:
    """

    results = []
    ignore_keys = ("type", "children")

    def flatten(dict_like, depth=None):
        depth = depth or 0
        node_type = dict_like["type"].lower()
        data = {
            f"{node_type}_{k}": v for k, v in dict_like.items() if k not in ignore_keys
        }
        if "children" in dict_like:
            for child in dict_like["children"]:
                for child_data in flatten(child, depth=depth + 1):
                    if depth == 0:
                        if all(child_data[k] == v for k, v in filters.items()):
                            results.append(child_data)
                    else:
                        yield {**data, **child_data}
        else:
            yield data

    list(flatten(y))
    return results


def chunk(list_like, n):
    """ Yield successive n-sized chunks from l."""
    for i in range(0, len(list_like), n):
        yield list_like[i : i + n]


def hash_json(data):
    h = hashlib.sha256(orjson.dumps(data))
    return h.hexdigest()

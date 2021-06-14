def list_dicts_to_dict_lists(dicts):
    result = {}
    for d in dicts:
        for k, v in d.items():
            if k not in result:
                result[k] = [v]
            else:
                result[k].append(v)
    return result


def maybe_list(dict_or_list):
    if isinstance(dict_or_list, dict):
        return [dict_or_list]
    return dict_or_list

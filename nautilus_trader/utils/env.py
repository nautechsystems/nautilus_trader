import os


def get_env_key(key: str):
    if key not in os.environ:
        raise RuntimeError(f"Cannot find env {key}")
    else:
        return os.environ[key]


def get_env_key_or(key: str, default: str):
    if key not in os.environ:
        return default
    else:
        return os.environ[key]

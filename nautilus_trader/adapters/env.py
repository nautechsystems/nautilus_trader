import os


def get_env_key(key: str) -> str:
    if key not in os.environ:
        raise RuntimeError(f"Environment variable '{key}' not set")
    else:
        return os.environ[key]


def get_env_key_or(key: str, default: str) -> str:
    if key not in os.environ:
        return default
    else:
        return os.environ[key]

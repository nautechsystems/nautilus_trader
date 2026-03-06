import os
from urllib.parse import urlparse

from nautilus_trader.core.nautilus_pyo3.network import http_download


def download_file(url: str):
    print(f"Checking file for {url}")
    path = url_to_path(url)
    print(f"Generated path: {path}")

    if os.path.exists(path):
        return path

    print(f"Downloading from {url}")
    headers = {"Authorization": f"Bearer {os.environ['TM_API_KEY']}"}
    http_download(url, path, headers=headers, timeout_secs=60)
    return path


def url_to_path(url: str) -> str:
    parsed_url = urlparse(url)
    path_components = [x for x in parsed_url.path.split("/") if x]

    exchange = path_components[1]
    data_type = path_components[2]
    year = path_components[3]
    month = path_components[4]
    day = path_components[5]
    filename = path_components[6]

    local_path = f"~/Downloads/tardis/{exchange}/{data_type}/{year}/{month}/{day}/{filename}"
    local_path = os.path.expanduser(local_path)
    return local_path

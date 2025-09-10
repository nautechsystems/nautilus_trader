# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import os
from urllib.parse import urlparse

import requests


def download_file(url: str):
    print(f"Checking file for {url}")
    path = url_to_path(url)
    print(f"Generated path: {path}")

    if os.path.exists(path):
        return path

    print(f"Downloading from {url}")
    os.makedirs(os.path.dirname(path), exist_ok=True)
    headers = {"Authorization": f"Bearer {os.environ['TM_API_KEY']}"}
    with requests.get(url, headers=headers, stream=True) as response:
        response.raise_for_status()
        with open(path, "wb") as file:
            for chunk in response.iter_content(chunk_size=8192):
                file.write(chunk)
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

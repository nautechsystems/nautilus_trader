# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

import logging
import os
import time

import dask
import pytest
from dask import compute
from dask import delayed
from distributed import Client

from nautilus_trader.persistence.external.synchronization import named_lock
from tests.test_kit import PACKAGE_ROOT


logger = logging.getLogger()

TEST_DATA = PACKAGE_ROOT + "/data"


@delayed
def _run():
    with named_lock("test.file"):
        with open("test.file", "ab") as f:
            f.write(b"hello")
            time.sleep(0.1)


@pytest.mark.parametrize("scheduler", ("sync", Client()))
def test_named_lock_sync(scheduler):
    if os.path.exists("test.file"):
        os.unlink("test.file")
    tasks = (_run(), _run(), _run())
    with dask.config.set(scheduler=scheduler):
        compute(tasks)
    r = open("test.file", "rb").read()
    assert r == b"hellohellohello"
    os.unlink("test.file")

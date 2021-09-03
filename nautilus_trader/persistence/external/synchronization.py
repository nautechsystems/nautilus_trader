# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import contextlib


try:
    import distributed
except ImportError:
    distributed = None


def running_on_dask() -> bool:
    try:
        from distributed import get_client

        get_client()
        return True
    except (ImportError, ValueError):
        return False


@contextlib.contextmanager
def distributed_lock(name):
    with distributed.Lock(name=name):
        yield


@contextlib.contextmanager
def named_lock(name):
    if running_on_dask():
        with distributed_lock(name=name):
            yield
    else:
        # Nothing to do - sync program
        yield

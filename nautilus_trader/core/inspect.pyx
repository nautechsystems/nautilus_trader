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

import gc
import sys

from nautilus_trader.core.data cimport Data
from nautilus_trader.core.message cimport Event


cpdef bint is_nautilus_class(cls):
    """
    Determine whether a class belongs to `nautilus_trader`.

    Defined as a `cdef` as closures not yet supported in `cpdef` functions.
    """
    cdef bint is_nautilus_paths = cls.__module__.startswith("nautilus_trader.")
    if not is_nautilus_paths:
        # This object is defined outside of Nautilus, definitely custom
        return False

    if issubclass(cls, (Data, Event)):
        return True

    cdef str cls_module = cls.__module__
    if (
        cls_module.startswith("nautilus_trader.model")
        or cls_module.startswith("nautilus_trader.adapters")
    ):
        return True

    return False


def get_size_of(obj):
    """
    Return the bytes size in memory of the given object.

    Parameters
    ----------
    obj : object
        The object to analyze.

    Returns
    -------
    uint64

    """
    cdef set marked = {id(obj)}
    obj_q = [obj]
    size = 0

    while obj_q:
        size += sum(map(sys.getsizeof, obj_q))

        # Lookup all the object referred to by the object in obj_q.
        # See: https://docs.python.org/3.7/library/gc.html#gc.get_referents
        all_refs = [(id(o), o) for o in gc.get_referents(*obj_q)]

        # Filter object that are already marked.
        # Using dict notation will prevent repeated objects.
        new_ref = {
            o_id: o for o_id, o in all_refs if o_id not in marked and not isinstance(o, type)
        }

        # The new obj_q will be the ones that were not marked,
        # and we will update marked with their ids so we will
        # not traverse them again.
        obj_q = new_ref.values()
        marked.update(new_ref.keys())

    return size

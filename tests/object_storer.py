#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="object_storer.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from typing import List


class ObjectStorer:
    """"
    A test class which stores the given objects.
    """
    def __init__(self):
        """
        Initializes a new instance of the ObjectStorer class.
        """
        self._store = []

    @property
    def get_store(self) -> List[object]:
        """"
        return: The internal object store.
        """
        return self._store

    def store(self, obj: object):
        """"
        Store the given object.
        """
        print(f"Storing {obj}")
        self._store.append(obj)

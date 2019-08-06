# -------------------------------------------------------------------------------------------------
# <copyright file="typed_collections.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.concurrency cimport FastRLock


cdef class TypedList:
    """
    Provides a strongly typed list.
    """

    def __init__(self, type type_value, list initial=None):
        """
        Initializes a new instance of the TypedList class.

        :param: type_value: The type of the list items.
        :param: initial: The initial items for the list.
        """
        assert type_value is not None, 'The type_value cannot be None'
        assert type_value is not type(None), 'The type_value type cannot be NoneType'
        if initial is None:
            initial = []
        elif len(initial) > 0:
            for i in range(len(initial)):
                assert isinstance(initial[i], self.type_value), f'The item at index {i} was not of type {type_value} (was {type(initial[i])})'

        self.type_value = type_value
        self._internal = initial

    def __len__(self) -> int:
        """
        Return the number of items in the list.

        :return: int.
        """
        return len(self._internal)

    def __getitem__(self, int index) -> object:
        """
        Return the item at the given index.

        :param index: The index of the item.
        :return: object.
        """
        return self._internal[index]

    def __setitem__(self, int index, x):
        """
        Set the item at the given index.

        :param index: The index of the item.
        :param x: The item to set.
        """
        assert isinstance(x, self.type_value), f'The item must be of type {self.type_value} (was {type(x)})'
        self._internal.__setitem__(index, x)

    def __delitem__(self, int index):
        """
        Return the item at the given index.

        :param index: The index of the item.
        :raises: KeyError: If the index is out of range.
        """
        self._internal.__delitem__(index)

    def __contains__(self, x) -> bool:
        """
        Returns a value indicating whether the list contains the given item.

        :param x: The item to contain.
        :return: bool.
        """
        assert isinstance(x, self.type_value), f'The item must be of type {self.type_value} (was {type(x)})'
        return self._internal.__contains__(x)

    cpdef void append(self, x):
        """
        Add the given item to the end of the list.
        
        :param x: The item to add.
        """
        assert isinstance(x, self.type_value), f'The item must be of type {self.type_value} (was {type(x)})'
        self._internal.append(x)

    cpdef void insert(self, int index, x):
        """
        Add the given item to the end of the list.
        
        :param index: The index to insert at.
        :param x: The item to add.
        """
        assert isinstance(x, self.type_value), f'The item must be of type {self.type_value} (was {type(x)})'
        self._internal.insert(index, x)

    cpdef void remove(self, x):
        """
        Remove the first item from the list whose value is equal to x.
        
        :param x: The item to remove.
        :raises: ValueError: If the list does not contain the item.
        """
        assert isinstance(x, self.type_value), f'The item must be of type {self.type_value} (was {type(x)})'
        self._internal.remove(x)

    cpdef object pop(self):
        """
        Remove the item from the end of the list, and return it. 
        """
        return self._internal.pop()

    cpdef object pop_at(self, int index):
        """
        Remove the item at the given position in the list, and return it. 
        
        :param index: The index to pop.
        """
        return self._internal.pop(index)

    cpdef void clear(self):
        """
        Remove all items from the list.
        """
        self._internal.clear()

    cpdef int index(self, x, int start, int stop):
        """
        Return zero-based index in the list of the first item whose value is equal to x.

        :param: x: The item for the index to find.
        :param: start: The start index for the search.
        :param: stop: The stop index for the search.
        :raises: ValueError: If the list does not contain the item.
        :return: int.
        """
        return self._internal.index(x, start, stop)

    cpdef int count(self, x):
        """
        Return the number of times x appears in the list.
        
        :param: x: The item for the search.
        :return: int.
        """
        return self._internal.count(x)

    cpdef void sort(self, key=None, bint reverse=False):
        """
        Sort the items of the list in place.
        
        :param: x: The item for the search.
        """
        self._internal.sort(key, reverse)

    cpdef void reverse(self):
        """
        Reverse the elements of the list in place.
        """
        self._internal.reverse()

    cpdef TypedList copy(self):
        """
        Return a shallow copy of the list.
        
        :return: TypedList.
        """
        return TypedList(self.type_value, self._internal.copy())

    cpdef void extend(self):
        """
        Not implemented for TypedList.
        """
        raise NotImplementedError()


cdef class TypedDictionary:
    """
    Provides a strongly typed dictionary.
    """

    def __init__(self, type type_key, type type_value, initial=None):
        """
        Initializes a new instance of the TypedDictionary class.

        :param: type_key: The type of the dictionary keys.
        :param: type_value: The type of the dictionary values.
        :param: initial: The initial items for the list.
        """
        assert type_key is not None, 'The type_key cannot be None'
        assert type_value is not None, 'The type_value type cannot be None'
        assert type_key is not type(None), 'The type_key type cannot be NoneType'
        assert type_value is not type(None), 'The type_value type cannot be NoneType'
        if initial is None:
            initial = {}
        elif len(initial) > 0:
            for k, v in initial.items():
                assert isinstance(k, self.type_key), f'The key must be of type {self.type_key} (was {type(k)})'
                assert isinstance(v, self.type_value), f'The value must be of type {self.type_value} (was {type(v)})'

        self.type_key = type_key
        self.type_value = type_value
        self._internal = initial

    def __len__(self):
        """
        Return the number of items in the dictionary.

        :return: int.
        """
        return len(self._internal)

    def __getitem__(self, k):
        assert isinstance(k, self.type_key), f'The key must be of type {self.type_key} (was {type(k)})'
        return self._internal.__getitem__(k)

    def __setitem__(self, k, v):
        assert isinstance(k, self.type_key), f'The key must be of type {self.type_key} (was {type(k)})'
        assert isinstance(v, self.type_value), f'The value must be of type {self.type_value} (was {type(v)})'
        self._internal.__setitem__(k, v)

    def __delitem__(self, k):
        assert isinstance(k, self.type_key), f'The key must be of type {self.type_key} (was {type(k)})'
        self._internal.__delitem__(k)

    def __contains__(self, k):
        assert isinstance(k, self.type_key), f'The key must be of type {self.type_key} (was {type(k)})'
        return self._internal.__contains__(k)

    cpdef object keys(self):
        return self._internal.keys()

    cpdef object values(self):
        return self._internal.values()

    cpdef object items(self):
        return self._internal.items()

    cpdef object get(self, k, default=None):
        assert isinstance(k, self.type_key), f'The key must be of type {self.type_key} (was {type(k)})'
        return self._internal.get(k, default)

    cpdef object setdefault(self, k, default=None):
        assert isinstance(k, self.type_key), f'The key must be of type {self.type_key} (was {type(k)})'
        return self._internal.setdefault(k, default)

    cpdef object pop(self, k, d=None):
        assert isinstance(k, self.type_key), f'The key must be of type {self.type_key} (was {type(k)})'
        return self._internal.pop(k, d)

    cpdef object popitem(self):
        return self._internal.popitem()

    cpdef dict copy(self):
        return self._internal.copy()

    cpdef void clear(self):
        self._internal.clear()


cdef class ConcurrentList:
    """
    Provides a strongly typed thread safe list.
    """

    def __init__(self, type type_value, list initial=None):
        """
        Initializes a new instance of the TypedList class.

        :param: type_value: The type of the list items.
        :param: initial: The initial items for the list.
        """
        self._lock = FastRLock()
        self._internal = TypedList(initial)

        self.type_value = type_value

    def __len__(self) -> int:
        """
        Return the number of items in the list.

        :return: int.
        """
        self._lock.acquire()
        cdef int length = len(self._internal)
        self._lock.release()
        return length

    def __getitem__(self, int index) -> object:
        """
        Return the item at the given index.

        :param index: The index of the item.
        :return: object.
        """
        self._lock.acquire()
        cdef int item_index = self._internal[index]
        self._lock.release()
        return item_index

    def __setitem__(self, int index, x):
        """
        Set the item at the given index.

        :param index: The index of the item.
        :param x: The item to set.
        """
        self._lock.acquire()
        self._internal.__setitem__(index, x)
        self._lock.release()

    def __delitem__(self, int index):
        """
        Return the item at the given index.

        :param index: The index of the item.
        :raises: KeyError: If the index is out of range.
        """
        self._lock.acquire()
        self._internal.__delitem__(index)
        self._lock.release()

    def __contains__(self, x) -> bool:
        """
        Returns a value indicating whether the list contains the given item.

        :param x: The item to contain.
        :return: bool.
        """
        self._lock.acquire()
        cdef bint contains = self._internal.__contains__(x)
        self._lock.release()
        return contains

    cpdef void append(self, x):
        """
        Add the given item to the end of the list.
        
        :param x: The item to add.
        """
        self._lock.acquire()
        self._internal.append(x)
        self._lock.release()

    cpdef void insert(self, int index, x):
        """
        Add the given item to the end of the list.
        
        :param index: The index to insert at.
        :param x: The item to add.
        """
        self._lock.acquire()
        self._internal.insert(index, x)
        self._lock.release()

    cpdef void remove(self, x):
        """
        Remove the first item from the list whose value is equal to x.
        
        :param x: The item to remove.
        :raises: ValueError: If the list does not contain the item.
        """
        self._lock.acquire()
        self._internal.remove(x)
        self._lock.release()

    cpdef object pop(self):
        """
        Remove the item from the end of the list, and return it. 
        """
        self._lock.acquire()
        cdef object popped = self._internal.pop()
        self._lock.release()
        return popped

    cpdef object pop_at(self, int index):
        """
        Remove the item at the given position in the list, and return it. 
        
        :param index: The index to pop.
        """
        self._lock.acquire()
        cdef object popped = self._internal.pop_at(index)
        self._lock.release()
        return popped

    cpdef void clear(self):
        """
        Remove all items from the list.
        """
        self._lock.acquire()
        self._internal.clear()
        self._lock.release()

    cpdef int index(self, x, int start, int stop):
        """
        Return zero-based index in the list of the first item whose value is equal to x.

        :param: x: The item for the index to find.
        :param: start: The start index for the search.
        :param: stop: The stop index for the search.
        :raises: ValueError: If the list does not contain the item.
        :return: int.
        """
        self._lock.acquire()
        cdef int index = self._internal.index(x, start, stop)
        self._lock.release()
        return index

    cpdef int count(self, x):
        """
        Return the number of times x appears in the list.
        
        :param: x: The item for the search.
        :return: int.
        """
        self._lock.acquire()
        cdef int count = self._internal.count(x)
        self._lock.release()
        return count

    cpdef void sort(self, key=None, bint reverse=False):
        """
        Sort the items of the list in place.
        
        :param: x: The item for the search.
        """
        self._lock.acquire()
        self._internal.sort(key, reverse)
        self._lock.release()

    cpdef void reverse(self):
        """
        Reverse the elements of the list in place.
        """
        self._lock.acquire()
        self._internal.reverse()
        self._lock.release()

    cpdef ConcurrentList copy(self):
        """
        Return a shallow copy of the list.
        
        :return: TypedList.
        """
        self._lock.acquire()
        cdef list copied = self._internal.copy()
        self._lock.release()
        return ConcurrentList(self.type_value, copied)

    cpdef void extend(self):
        """
        Not implemented for TypedList.
        """
        raise NotImplementedError()


cdef class ConcurrentDictionary:
    """
    Provides a strongly typed thread safe dictionary.
    """

    def __init__(self, type type_key, type type_value):
        """
        Initializes a new instance of the ConcurrentDictionary class.
        """
        self._lock = FastRLock()
        self._internal = TypedDictionary(type_key, type_value)

        self.type_key = type_key
        self.type_value = type_value

    def __len__(self):
        """
        Return the number of items in the dictionary.

        :return: int.
        """
        self._lock.acquire()
        cdef int length = len(self._internal)
        self._lock.release()
        return length

    def __enter__(self):
        """
        Context manager enter the block, acquire the lock.
        """
        self._lock.acquire()
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        """
        Context manager exit the block, release the lock.
        """
        self._lock.release()

    def __getitem__(self, k):
        self._lock.acquire()
        item = self._internal.__getitem__(k)
        self._lock.release()
        return item

    def __setitem__(self, k, v):
        self._lock.acquire()
        self._internal.__setitem__(k, v)
        self._lock.release()

    def __delitem__(self, k):
        self._lock.acquire()
        self._internal.__delitem__(k)
        self._lock.release()

    def __contains__(self, k):
        self._lock.acquire()
        result = self._internal.__contains__(k)
        self._lock.release()
        return result

    cpdef object keys(self):
        self._lock.acquire()
        keys = self._internal.keys()
        self._lock.release()
        return keys

    cpdef object values(self):
        self._lock.acquire()
        values = self._internal.values()
        self._lock.release()
        return values

    cpdef object items(self):
        self._lock.acquire()
        items = self._internal.items()
        self._lock.release()
        return items

    cpdef object get(self, k, default=None):
        self._lock.acquire()
        item = self._internal.get(k, default)
        self._lock.release()
        return item

    cpdef object setdefault(self, k, default=None):
        self._lock.acquire()
        result = self._internal.setdefault(k, default)
        self._lock.release()
        return result

    cpdef object pop(self, k, d=None):
        self._lock.acquire()
        item = self._internal.pop(k, d)
        self._lock.release()
        return item

    cpdef object popitem(self):
        self._lock.acquire()
        item = self._internal.popitem()
        self._lock.release()
        return item

    cpdef dict copy(self):
        self._lock.acquire()
        copied = self._internal.copy()
        self._lock.release()
        return copied

    cpdef void clear(self):
        self._lock.acquire()
        self._internal.clear()
        self._lock.release()


cdef class ObjectCache:
    """
    Provides a strongly typed object cache with strings as keys.
    """

    def __init__(self, type type_value, parser):
        """
        Initializes a new instance of the ObjectCache class.
        """
        assert type_value is not None, 'The type_value type cannot be None'
        assert type_value is not type(None), 'The type_value type cannot be NoneType'

        self.type_key = str
        self.type_value = type_value
        self._cache = ConcurrentDictionary(str, type_value)
        self._parser = parser

    cpdef object get(self, str key):
        """
        Return the cached object for the given key otherwise cache and return
        the parsed key.

        :param key: The key to check.
        :return: object.
        """
        parsed = self._cache.get(key, None)

        if parsed is None:
            parsed = self._parser(key)
            self._cache[key] = parsed

        return parsed

    cpdef void clear(self):
        """
        Clears all cached values.
        """
        self._cache.clear()

/*
Copyright (c) 2005-2008, Simon Howard
Permission to use, copy, modify, and/or distribute this software
for any purpose with or without fee is hereby granted, provided
that the above copyright notice and this permission notice appear
in all copies.
THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL
WARRANTIES WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED
WARRANTIES OF MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE
AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT, INDIRECT, OR
CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM
LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT,
NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN
CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */

/**
 * @file list.h
 *
 * @brief Doubly-linked list.
 *
 * A doubly-linked list stores a collection of values.  Each entry in
 * the list (represented by a pointer a @ref ListEntry structure)
 * contains a link to the next entry and the previous entry.
 * It is therefore possible to iterate over entries in the list in either
 * direction.
 *
 * To create an empty list, create a new variable which is a pointer to
 * a @ref ListEntry structure, and initialise it to NULL.
 * To destroy an entire list, use @ref list_free.
 *
 * To add a value to a list, use @ref list_append or @ref list_prepend.
 *
 * To remove a value from a list, use @ref list_remove_entry or
 * @ref list_remove_data.
 *
 * To iterate over entries in a list, use @ref list_iterate to initialise
 * a @ref ListIterator structure, with @ref list_iter_next and
 * @ref list_iter_has_more to retrieve each value in turn.
 * @ref list_iter_remove can be used to remove the current entry.
 *
 * To access an entry in the list by index, use @ref list_nth_entry or
 * @ref list_nth_data.
 *
 * To modify data in the list use @ref list_set_data.
 *
 * To sort a list, use @ref list_sort.
 *
 */

#ifndef ALGORITHM_LIST_H
#define ALGORITHM_LIST_H

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Represents an entry in a doubly-linked list.  The empty list is
 * represented by a NULL pointer. To initialise a new doubly linked
 * list, simply create a variable of this type
 * containing a pointer to NULL.
 */

typedef struct _ListEntry ListEntry;

/**
 * Structure used to iterate over a list.
 */

typedef struct _ListIterator ListIterator;

/**
 * A value stored in a list.
 */

typedef void *ListValue;

/**
 * Definition of a @ref ListIterator.
 */

struct _ListIterator {
	ListEntry **prev_next;
	ListEntry *current;
};

/**
 * A null @ref ListValue.
 */

#define LIST_NULL ((void *) 0)

/**
 * Callback function used to compare values in a list when sorting.
 *
 * @param value1      The first value to compare.
 * @param value2      The second value to compare.
 * @return            A negative value if value1 should be sorted before
 *                    value2, a positive value if value1 should be sorted
 *                    after value2, zero if value1 and value2 are equal.
 */

typedef int (*ListCompareFunc)(ListValue value1, ListValue value2);

/**
 * Callback function used to determine of two values in a list are
 * equal.
 *
 * @param value1      The first value to compare.
 * @param value2      The second value to compare.
 * @return            A non-zero value if value1 and value2 are equal, zero
 *                    if they are not equal.
 */

typedef int (*ListEqualFunc)(ListValue value1, ListValue value2);

/**
 * Free an entire list.
 *
 * @param list         The list to free.
 */

void list_free(ListEntry *list);

/**
 * Prepend a value to the start of a list.
 *
 * @param list         Pointer to the list to prepend to.
 * @param data         The value to prepend.
 * @return             The new entry in the list, or NULL if it was not
 *                     possible to allocate the memory for the new entry.
 */

ListEntry *list_prepend(ListEntry **list, ListValue data);

/**
 * Append a value to the end of a list.
 *
 * @param list         Pointer to the list to append to.
 * @param data         The value to append.
 * @return             The new entry in the list, or NULL if it was not
 *                     possible to allocate the memory for the new entry.
 */

ListEntry *list_append(ListEntry **list, ListValue data);

/**
 * Retrieve the previous entry in a list.
 *
 * @param listentry    Pointer to the list entry.
 * @return             The previous entry in the list, or NULL if this
 *                     was the first entry in the list.
 */

ListEntry *list_prev(ListEntry *listentry);

/**
 * Retrieve the next entry in a list.
 *
 * @param listentry    Pointer to the list entry.
 * @return             The next entry in the list, or NULL if this was the
 *                     last entry in the list.
 */

ListEntry *list_next(ListEntry *listentry);

/**
 * Retrieve the value at a list entry.
 *
 * @param listentry    Pointer to the list entry.
 * @return             The value stored at the list entry.
 */

ListValue list_data(ListEntry *listentry);

/**
 * Set the value at a list entry. The value provided will be written to the
 * given listentry. If listentry is NULL nothing is done.
 *
 * @param listentry 	Pointer to the list entry.
 * @param value			The value to set.
 */
void list_set_data(ListEntry *listentry, ListValue value);

/**
 * Retrieve the entry at a specified index in a list.
 *
 * @param list       The list.
 * @param n          The index into the list .
 * @return           The entry at the specified index, or NULL if out of range.
 */

ListEntry *list_nth_entry(ListEntry *list, unsigned int n);

/**
 * Retrieve the value at a specified index in the list.
 *
 * @param list       The list.
 * @param n          The index into the list.
 * @return           The value at the specified index, or @ref LIST_NULL if
 *                   unsuccessful.
 */

ListValue list_nth_data(ListEntry *list, unsigned int n);

/**
 * Find the length of a list.
 *
 * @param list       The list.
 * @return           The number of entries in the list.
 */

unsigned int list_length(ListEntry *list);

/**
 * Create a C array containing the contents of a list.
 *
 * @param list       The list.
 * @return           A newly-allocated C array containing all values in the
 *                   list, or NULL if it was not possible to allocate the
 *                   memory.  The length of the array is equal to the length
 *                   of the list (see @ref list_length).
 */

ListValue *list_to_array(ListEntry *list);

/**
 * Remove an entry from a list.
 *
 * @param list       Pointer to the list.
 * @param entry      The list entry to remove .
 * @return           If the entry is not found in the list, returns zero,
 *                   else returns non-zero.
 */

int list_remove_entry(ListEntry **list, ListEntry *entry);

/**
 * Remove all occurrences of a particular value from a list.
 *
 * @param list       Pointer to the list.
 * @param callback   Function to invoke to compare values in the list
 *                   with the value to be removed.
 * @param data       The value to remove from the list.
 * @return           The number of entries removed from the list.
 */

unsigned int list_remove_data(ListEntry **list, ListEqualFunc callback,
                              ListValue data);

/**
 * Sort a list.
 *
 * @param list          Pointer to the list to sort.
 * @param compare_func  Function used to compare values in the list.
 */

void list_sort(ListEntry **list, ListCompareFunc compare_func);

/**
 * Find the entry for a particular value in a list.
 *
 * @param list           The list to search.
 * @param callback       Function to invoke to compare values in the list
 *                       with the value to be searched for.
 * @param data           The value to search for.
 * @return               The list entry of the item being searched for, or
 *                       NULL if not found.
 */

ListEntry *list_find_data(ListEntry *list,
                          ListEqualFunc callback,
                          ListValue data);

/**
 * Initialise a @ref ListIterator structure to iterate over a list.
 *
 * @param list           A pointer to the list to iterate over.
 * @param iter           A pointer to an iterator structure to initialise.
 */

void list_iterate(ListEntry **list, ListIterator *iter);

/**
 * Determine if there are more values in the list to iterate over.
 *
 * @param iterator       The list iterator.
 * @return               Zero if there are no more values in the list to
 *                       iterate over, non-zero if there are more values to
 *                       read.
 */

int list_iter_has_more(ListIterator *iterator);

/**
 * Using a list iterator, retrieve the next value from the list.
 *
 * @param iterator       The list iterator.
 * @return               The next value from the list, or @ref LIST_NULL if
 *                       there are no more values in the list.
 */

ListValue list_iter_next(ListIterator *iterator);

/**
 * Delete the current entry in the list (the value last returned from
 * list_iter_next)
 *
 * @param iterator       The list iterator.
 */

void list_iter_remove(ListIterator *iterator);

#ifdef __cplusplus
}
#endif

#endif /* #ifndef ALGORITHM_LIST_H */

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

#include <stdlib.h>

#include "list.h"

/* malloc() / free() testing */

#ifdef ALLOC_TESTING
#include "alloc-testing.h"
#endif

/* A doubly-linked list */

struct _ListEntry {
	ListValue data;
	ListEntry *prev;
	ListEntry *next;
};

void list_free(ListEntry *list)
{
	ListEntry *entry;

	/* Iterate over each entry, freeing each list entry, until the
	 * end is reached */

	entry = list;

	while (entry != NULL) {
		ListEntry *next;

		next = entry->next;

		free(entry);

		entry = next;
	}
}

ListEntry *list_prepend(ListEntry **list, ListValue data)
{
	ListEntry *newentry;

	if (list == NULL) {

		/* not a valid list */

		return NULL;
	}

	/* Create new entry */

	newentry = malloc(sizeof(ListEntry));

	if (newentry == NULL) {
		return NULL;
	}

	newentry->data = data;

	/* Hook into the list start */

	if (*list != NULL) {
		(*list)->prev = newentry;
	}
	newentry->prev = NULL;
	newentry->next = *list;
	*list = newentry;

	return newentry;
}

ListEntry *list_append(ListEntry **list, ListValue data)
{
	ListEntry *rover;
	ListEntry *newentry;

	if (list == NULL) {
		return NULL;
	}

	/* Create new list entry */

	newentry = malloc(sizeof(ListEntry));

	if (newentry == NULL) {
		return NULL;
	}

	newentry->data = data;
	newentry->next = NULL;

	/* Hooking into the list is different if the list is empty */

	if (*list == NULL) {

		/* Create the start of the list */

		*list = newentry;
		newentry->prev = NULL;

	} else {

		/* Find the end of list */

		for (rover=*list; rover->next != NULL; rover = rover->next);

		/* Add to the end of list */

		newentry->prev = rover;
		rover->next = newentry;
	}

	return newentry;
}

ListValue list_data(ListEntry *listentry)
{
	if (listentry == NULL) {
		return LIST_NULL;
	}

	return listentry->data;
}

void list_set_data(ListEntry *listentry, ListValue value)
{
	if (listentry != NULL) {
		listentry->data = value;
	}
}

ListEntry *list_prev(ListEntry *listentry)
{
	if (listentry == NULL) {
		return NULL;
	}

	return listentry->prev;
}

ListEntry *list_next(ListEntry *listentry)
{
	if (listentry == NULL) {
		return NULL;
	}

	return listentry->next;
}

ListEntry *list_nth_entry(ListEntry *list, unsigned int n)
{
	ListEntry *entry;
	unsigned int i;

	/* Iterate through n list entries to reach the desired entry.
	 * Make sure we do not reach the end of the list. */

	entry = list;

	for (i=0; i<n; ++i) {

		if (entry == NULL) {
			return NULL;
		}
		entry = entry->next;
	}

	return entry;
}

ListValue list_nth_data(ListEntry *list, unsigned int n)
{
	ListEntry *entry;

	/* Find the specified entry */

	entry = list_nth_entry(list, n);

	/* If out of range, return NULL, otherwise return the data */

	if (entry == NULL) {
		return LIST_NULL;
	} else {
		return entry->data;
	}
}

unsigned int list_length(ListEntry *list)
{
	ListEntry *entry;
	unsigned int length;

	length = 0;
	entry = list;

	while (entry != NULL) {

		/* Count the number of entries */

		++length;

		entry = entry->next;
	}

	return length;
}

ListValue *list_to_array(ListEntry *list)
{
	ListEntry *rover;
	ListValue *array;
	unsigned int length;
	unsigned int i;

	/* Allocate an array equal in size to the list length */

	length = list_length(list);

	array = malloc(sizeof(ListValue) * length);

	if (array == NULL) {
		return NULL;
	}

	/* Add all entries to the array */

	rover = list;

	for (i=0; i<length; ++i) {

		/* Add this node's data */

		array[i] = rover->data;

		/* Jump to the next list node */

		rover = rover->next;
	}

	return array;
}

int list_remove_entry(ListEntry **list, ListEntry *entry)
{
	/* If the list is empty, or entry is NULL, always fail */

	if (list == NULL || *list == NULL || entry == NULL) {
		return 0;
	}

	/* Action to take is different if the entry is the first in the list */

	if (entry->prev == NULL) {

		/* Unlink the first entry and update the starting pointer */

		*list = entry->next;

		/* Update the second entry's prev pointer, if there is a second
		 * entry */

		if (entry->next != NULL) {
			entry->next->prev = NULL;
		}

	} else {

		/* This is not the first in the list, so we must have a
		 * previous entry.  Update its 'next' pointer to the new
		 * value */

		entry->prev->next = entry->next;

		/* If there is an entry following this one, update its 'prev'
		 * pointer to the new value */

		if (entry->next != NULL) {
			entry->next->prev = entry->prev;
		}
	}

	/* Free the list entry */

	free(entry);

	/* Operation successful */

	return 1;
}

unsigned int list_remove_data(ListEntry **list, ListEqualFunc callback,
                              ListValue data)
{
	unsigned int entries_removed;
	ListEntry *rover;
	ListEntry *next;

	if (list == NULL || callback == NULL) {
		return 0;
	}

	entries_removed = 0;

	/* Iterate over the entries in the list */

	rover = *list;

	while (rover != NULL) {

		next = rover->next;

		if (callback(rover->data, data)) {

			/* This data needs to be removed.  Unlink this entry
			 * from the list. */

			if (rover->prev == NULL) {

				/* This is the first entry in the list */

				*list = rover->next;
			} else {

				/* Point the previous entry at its new
				 * location */

				rover->prev->next = rover->next;
			}

			if (rover->next != NULL) {
				rover->next->prev = rover->prev;
			}

			/* Free the entry */

			free(rover);

			++entries_removed;
		}

		/* Advance to the next list entry */

		rover = next;
	}

	return entries_removed;
}

/* Function used internally for sorting.  Returns the last entry in the
 * new sorted list */

static ListEntry *list_sort_internal(ListEntry **list,
                                     ListCompareFunc compare_func)
{
	ListEntry *pivot;
	ListEntry *rover;
	ListEntry *less_list, *more_list;
	ListEntry *less_list_end, *more_list_end;

	if (list == NULL || compare_func == NULL) {
		return NULL;
	}

	/* If there are less than two entries in this list, it is
	 * already sorted */

	if (*list == NULL || (*list)->next == NULL) {
		return *list;
	}

	/* The first entry is the pivot */

	pivot = *list;

	/* Iterate over the list, starting from the second entry.  Sort
	 * all entries into the less and more lists based on comparisons
	 * with the pivot */

	less_list = NULL;
	more_list = NULL;
	rover = (*list)->next;

	while (rover != NULL) {
		ListEntry *next = rover->next;

		if (compare_func(rover->data, pivot->data) < 0) {

			/* Place this in the less list */

			rover->prev = NULL;
			rover->next = less_list;
			if (less_list != NULL) {
				less_list->prev = rover;
			}
			less_list = rover;

		} else {

			/* Place this in the more list */

			rover->prev = NULL;
			rover->next = more_list;
			if (more_list != NULL) {
				more_list->prev = rover;
			}
			more_list = rover;
		}

		rover = next;
	}

	/* Sort the sublists recursively */

	less_list_end = list_sort_internal(&less_list, compare_func);
	more_list_end = list_sort_internal(&more_list, compare_func);

	/* Create the new list starting from the less list */

	*list = less_list;

	/* Append the pivot to the end of the less list.  If the less list
	 * was empty, start from the pivot */

	if (less_list == NULL) {
		pivot->prev = NULL;
		*list = pivot;
	} else {
		pivot->prev = less_list_end;
		less_list_end->next = pivot;
	}

	/* Append the more list after the pivot */

	pivot->next = more_list;
	if (more_list != NULL) {
		more_list->prev = pivot;
	}

	/* Work out what the last entry in the list is.  If the more list was
	 * empty, the pivot was the last entry.  Otherwise, the end of the
	 * more list is the end of the total list. */

	if (more_list == NULL) {
		return pivot;
	} else {
		return more_list_end;
	}
}

void list_sort(ListEntry **list, ListCompareFunc compare_func)
{
	list_sort_internal(list, compare_func);
}

ListEntry *list_find_data(ListEntry *list,
                          ListEqualFunc callback,
                          ListValue data)
{
	ListEntry *rover;

	/* Iterate over entries in the list until the data is found */

	for (rover=list; rover != NULL; rover=rover->next) {
		if (callback(rover->data, data) != 0) {
			return rover;
		}
	}

	/* Not found */

	return NULL;
}

void list_iterate(ListEntry **list, ListIterator *iter)
{
	/* Start iterating from the beginning of the list. */

	iter->prev_next = list;

	/* We have not yet read the first item. */

	iter->current = NULL;
}

int list_iter_has_more(ListIterator *iter)
{
	if (iter->current == NULL || iter->current != *iter->prev_next) {

		/* Either we have not read the first entry, the current
		 * item was removed or we have reached the end of the
		 * list.  Use prev_next to determine if we have a next
		 * value to iterate over. */

		return *iter->prev_next != NULL;

	} else {
		/* The current entry as not been deleted since the last
		 * call to list_iter_next: there is a next entry if
		 * current->next is not NULL */

		return iter->current->next != NULL;
	}
}

ListValue list_iter_next(ListIterator *iter)
{
	if (iter->current == NULL || iter->current != *iter->prev_next) {

		/* Either we are reading the first entry, we have reached
		 * the end of the list, or the previous entry was removed.
		 * Get the next entry with iter->prev_next. */

		iter->current = *iter->prev_next;

	} else {

		/* Last value returned from list_iter_next was not deleted.
		 * Advance to the next entry. */

		iter->prev_next = &iter->current->next;
		iter->current = iter->current->next;
	}

	/* Have we reached the end of the list? */

	if (iter->current == NULL) {
		return LIST_NULL;
	} else {
		return iter->current->data;
	}
}

void list_iter_remove(ListIterator *iter)
{
	if (iter->current == NULL || iter->current != *iter->prev_next) {

		/* Either we have not yet read the first item, we have
		 * reached the end of the list, or we have already removed
		 * the current value.  Either way, do nothing. */

	} else {

		/* Remove the current entry */

		*iter->prev_next = iter->current->next;

		if (iter->current->next != NULL) {
			iter->current->next->prev = iter->current->prev;
		}

		free(iter->current);
		iter->current = NULL;
	}
}

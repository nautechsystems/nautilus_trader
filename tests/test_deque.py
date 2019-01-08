#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_deque.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import datetime
import unittest

from collections import deque

from inv_trader.core.deque import Deque, DequeDouble


class DequeTests(unittest.TestCase):


    def test_can_append_right(self):
        # Arrange
        deque = Deque(maxlen=3)

        # Act
        deque.appendright('A')
        deque.appendright('B')
        deque.appendright('C')
        deque.appendright('D')

        # Assert
        self.assertEqual(['B', 'C', 'D'], deque)

    def test_can_append_left(self):
        # Arrange
        deque = Deque(maxlen=3)

        # Act
        deque.appendleft('A')
        deque.appendleft('B')
        deque.appendleft('C')
        deque.appendleft('D')

        # Assert
        self.assertEqual(['D', 'C', 'B'], deque)

    def test_is_empty_returns_expected(self):
        # Arrange
        deque1 = Deque(maxlen=3)
        deque2 = Deque(maxlen=3)
        deque2.append(1.0)
        deque3 = Deque(1)
        deque3.append(1.0)
        deque3.pop()

        # Act
        result1 = deque1.is_empty()
        result2 = deque2.is_empty()
        result3 = deque3.is_empty()

        # Assert
        self.assertTrue(result1)
        self.assertFalse(result2)
        self.assertTrue(result3)


class DequeDoubleTests(unittest.TestCase):


    def test_can_append_right(self):
        # Arrange
        deque = DequeDouble(maxlen=3)

        # Act
        deque.appendright(1.0)
        deque.appendright(2.0)
        deque.appendright(3.0)
        deque.appendright(4.0)

        # Assert
        self.assertEqual([2.0, 3.0, 4.0], deque)

    def test_can_append_left(self):
        # Arrange
        deque = DequeDouble(maxlen=3)

        # Act
        deque.appendleft(1.0)
        deque.appendleft(2.0)
        deque.appendleft(3.0)
        deque.appendleft(4.0)

        # Assert
        self.assertEqual([4.0, 3.0, 2.0], deque)

    def test_is_empty_returns_expected(self):
        # Arrange
        deque1 = DequeDouble(maxlen=3)
        deque2 = DequeDouble(maxlen=3)
        deque2.append(1.0)
        deque3 = DequeDouble(1)
        deque3.append(1.0)
        deque3.pop()

        # Act
        result1 = deque1.is_empty()
        result2 = deque2.is_empty()
        result3 = deque3.is_empty()

        # Assert
        self.assertTrue(result1)
        self.assertFalse(result2)
        self.assertTrue(result3)

    def test_performance(self):
        iterations = 1000000
        deque_double = DequeDouble(maxlen=10)
        deque_stock = deque(maxlen=10)
        list = []

        # Test DequeDouble
        start_deque_double = datetime.datetime.now()
        for x in range(iterations):
            deque_double.append(1.0)
        stop_deque_double = datetime.datetime.now()

        # Test Deque
        start_deque_stock = datetime.datetime.now()
        for x in range(iterations):
            deque_stock.append(1.0)
        stop_deque_stock = datetime.datetime.now()

        # Test List
        start_list = datetime.datetime.now()
        for x in range(iterations):
            list.append(1.0)
        stop_list = datetime.datetime.now()

        print('\n')
        print(f'DequeDouble performance on {iterations} iterations = {stop_deque_double - start_deque_double}')
        print(f'Deque performance on {iterations} iterations = {stop_deque_stock - start_deque_stock}')
        print(f'List performance on {iterations} iterations = {stop_list - start_list}')

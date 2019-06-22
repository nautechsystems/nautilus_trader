#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_backtest_loaders.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from decimal import Decimal

from inv_trader.enums import Venue
from inv_trader.model.objects import Symbol
from inv_trader.backtest.loaders import InstrumentLoader


class BacktestLoadersTests(unittest.TestCase):

    def test_default_fx_with_5_dp_returns_expected_instrument(self):
        # Arrange
        loader = InstrumentLoader()

        # Act
        instrument = loader.default_fx_ccy(Symbol('AUDUSD', Venue.DUKASCOPY), tick_precision=5)

        # Assert
        self.assertEqual(Symbol('AUDUSD', Venue.DUKASCOPY), instrument.symbol)
        self.assertEqual('AUD/USD', instrument.broker_symbol)
        self.assertEqual(5, instrument.tick_precision)
        self.assertEqual(Decimal('0.00001'), instrument.tick_size)
        self.assertEqual(840, instrument.quote_currency)

    def test_default_fx_with_3_dp_returns_expected_instrument(self):
        # Arrange
        loader = InstrumentLoader()

        # Act
        instrument = loader.default_fx_ccy(Symbol('USDJPY', Venue.DUKASCOPY), tick_precision=3)

        # Assert
        self.assertEqual(Symbol('USDJPY', Venue.DUKASCOPY), instrument.symbol)
        self.assertEqual('USD/JPY', instrument.broker_symbol)
        self.assertEqual(3, instrument.tick_precision)
        self.assertEqual(Decimal('0.001'), instrument.tick_size)
        self.assertEqual(392, instrument.quote_currency)

# -------------------------------------------------------------------------------------------------
# <copyright file="models.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import random

from nautilus_trader.core.precondition cimport Precondition


cdef class FillModel:
    """
    Provides probabilistic modeling for order fill dynamics including probability
    of fills and slippage by order type.
    """

    def __init__(self,
                 float prob_fill_at_limit=0.0,
                 float prob_fill_at_stop=1.0,
                 float prob_slippage=0.0,
                 random_seed=None):
        """
        Initializes a new instance of the FillModel class.

        :param prob_fill_at_limit: The probability of limit order filling if the market rests on their price.
        :param prob_fill_at_stop: The probability of stop orders filling if the market rests on their price.
        :param prob_slippage: The probability of order fill prices slipping by a tick.
        :param random_seed: The random seed (optional can be None - no random seed).
        :raises ValueError: If any probability argument is not within range [0, 1].
        :raises ValueError: If the random_seed is not None and not of type int.
        """
        Precondition.in_range(prob_fill_at_limit, 'prob_fill_at_limit', 0.0, 1.0)
        Precondition.in_range(prob_fill_at_stop, 'prob_fill_at_stop', 0.0, 1.0)
        Precondition.in_range(prob_slippage, 'prob_slippage', 0.0, 1.0)
        if random_seed is not None:
            Precondition.type(random_seed, int, 'random_seed')

        self.prob_fill_at_limit = prob_fill_at_limit
        self.prob_fill_at_stop = prob_fill_at_stop
        self.prob_slippage = prob_slippage
        random.seed(random_seed)

    cpdef bint is_limit_filled(self):
        """
        Return the models outcome for the probability of a LIMIT order filling.
        
        :return: True if the event occurred, else False.
        """
        return self._did_event_occur(self.prob_fill_at_limit)

    cpdef bint is_stop_filled(self):
        """
        Return the models outcome for the probability of a STOP order filling.
        
        :return: True if the event occurred, else False.
        """
        return self._did_event_occur(self.prob_fill_at_stop)

    cpdef bint is_slipped(self):
        """
        Return the models outcome for the probability of an order fill slipping.
        
        :return: True if the event occurred, else False.
        """
        return self._did_event_occur(self.prob_slippage)

    cdef bint _did_event_occur(self, float probability):
        # Return a result indicating whether an event occurred based on the
        # given probability.
        
        # :param probability: The probability of the event occurring [0, 1].
        if probability == 0.0:
            return False
        elif probability == 1.0:
            return True
        else:
            return probability >= random.random()

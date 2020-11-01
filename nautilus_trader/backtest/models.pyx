# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core.correctness cimport Condition


# C Standard Library
cdef extern from "stdlib.h":
    double drand48()  # Returns a double in range [0,1)
    void srand48(long int seedval)

cdef extern from "time.h":
    long int time(int)


cdef class FillModel:
    """
    Provides probabilistic modeling for order fill dynamics including probability
    of fills and slippage by order type.
    """

    def __init__(
            self,
            double prob_fill_at_limit=0.0,
            double prob_fill_at_stop=1.0,
            double prob_slippage=0.0,
            random_seed=None,
    ):
        """
        Initialize a new instance of the `FillModel` class.

        Parameters
        ----------
        prob_fill_at_limit : double
            The probability of limit order filling if the market rests on their price.
        prob_fill_at_stop : double
            The probability of stop orders filling if the market rests on their price.
        prob_slippage : double
            The probability of order fill prices slipping by a tick.
        random_seed :
            The optional random seed (if None then no random seed).

        Raises
        ------
        ValueError
            If any probability argument is not within range [0, 1].
        TypeError
            If random_seed is not None and not of type int.

        """
        Condition.in_range(prob_fill_at_limit, 0.0, 1.0, "prob_fill_at_limit")
        Condition.in_range(prob_fill_at_stop, 0.0, 1.0, "prob_fill_at_stop")
        Condition.in_range(prob_slippage, 0.0, 1.0, "prob_slippage")
        if random_seed:
            Condition.type(random_seed, int, "random_seed")
        else:
            random_seed = 42

        self.prob_fill_at_limit = prob_fill_at_limit
        self.prob_fill_at_stop = prob_fill_at_stop
        self.prob_slippage = prob_slippage
        srand48(random_seed)

    cpdef bint is_limit_filled(self) except *:
        """
        Return the models outcome for the probability of a LIMIT order filling.

        Returns
        -------
        bool

        """
        return self._did_event_occur(self.prob_fill_at_limit)

    cpdef bint is_stop_filled(self) except *:
        """
        Return the models outcome for the probability of a STOP order filling.

        Returns
        -------
        bool

        """
        return self._did_event_occur(self.prob_fill_at_stop)

    cpdef bint is_slipped(self) except *:
        """
        Return the models outcome for the probability of an order fill slipping.

        Returns
        -------
        bool

        """
        return self._did_event_occur(self.prob_slippage)

    cdef bint _did_event_occur(self, double probability) except *:
        # Return a result indicating whether an event occurred based on the
        # given probability.

        # probability is the probability of the event occurring [0, 1].
        if probability == 0.0:
            return False
        elif probability == 1.0:
            return True
        else:
            return probability >= drand48()

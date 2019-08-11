# -------------------------------------------------------------------------------------------------
# <copyright file="node.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import zmq

from nautilus_trader.core.correctness cimport Condition


cdef class TradingNode:
    """
    Provides a trading node which provides an external API.
    """

    def __init__(
            self,
            str config_path='config.json'):
        """
        Initializes a new instance of the TradingNode class.

        :param config_path: The path to the config file.
        :raises ValueError: If the config_path is not a valid string.
        """
        Condition.valid_string(config_path, 'config_path')

        self._zmq_context = zmq.Context()


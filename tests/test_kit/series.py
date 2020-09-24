# -------------------------------------------------------------------------------------------------
# <copyright file="series.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 202018-201918 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from random import gauss
import sys

import numpy as np
from scipy import signal

EPSILON = sys.float_info.epsilon


class SeriesGenerator(object):

    @staticmethod
    def step_function(
            initial: float=EPSILON,
            magnitude: float=1.0,
            count_pre: int=0,
            count_post: int=0,
    ) -> np.array:
        """
        Generate a series with a heaviside step function.

        :param initial: The initial starting value (>= 0).
        :param magnitude: The step function magnitude (>= 0).
        :param count_pre: The number of elements prior to the step (>= 0).
        :param count_post: The number of elements after the step (>= 0).
        :return: The step function series.
        """
        return np.append(
            np.full(max(count_pre, 1), initial, dtype=np.float64),
            np.full(max(count_post, 1), magnitude, dtype=np.float64),
        )

    @staticmethod
    def spike_function(
            initial: float=EPSILON,
            magnitude: float=1.0,
            count_pre: int=0,
            count_post: int=0,
    ) -> np.array:
        """
        Generate a series with a spike function.

        :param initial: The initial starting value (>= 0).
        :param magnitude: The spike magnitude (>= 0).
        :param count_pre: The number of elements prior to the spike (>= 0).
        :param count_post: The number of elements after the spike (>= 0).
        :return: The spike function series.
        """
        return np.append(
            np.append(
                np.full(max(count_pre, 1), initial, dtype=np.float64),
                [magnitude]),
            np.full(max(count_post, 1), initial, dtype=np.float64),
        )

    @staticmethod
    def horizontal_asymptote(
            initial: float=0.00100,
            decay: float=0.98,
            length: int=1000,
    ) -> np.array:
        """
        Generate a horizontally asymptotic series.

        :param initial: The initial starting value (> 0).
        :param decay: The decay rate (> 0).
        :param length: The number of elements in the returned series (> 0).
        :return: The horizontally asymptotic series.
        """
        series = [initial]
        for _i in range(length - 1):
            series.append(max(series[-1] * decay, EPSILON))
        return series

    @staticmethod
    def sine_wave(
            initial: float=1.00000,
            magnitude: float=0.00100,
            length: int=1000,
    ) -> np.array:
        """
        Generate a sine wave series.

        :param initial: The initial starting value (> 0).
        :param magnitude: The sine wave magnitude (> 0).
        :param length: The number of elements in the returned series (> 0).
        :return: The sine wave series.
        """
        return np.sin(2 * np.pi * np.arange(length) / (length / 2)) * magnitude + initial

    @staticmethod
    def sawtooth(
            frequency: float=1.0,
            length: int=1000,
    ) -> np.array:
        """
        Generate a sawtooth signal series [-1.0, 1.0].

        :param frequency: The frequency of oscillations (> 0).
        :param length: The number of elements in the returned series (> 0).
        :return: The sawtooth series.
        """
        t = np.linspace(0., frequency, length)
        return np.array(signal.sawtooth(2 * np.pi * 5 * t))

    @staticmethod
    def white_noise(
            mu: float=0.0,
            sigma: float=1.0,
            length: int =1000,
    ) -> np.array:
        """
        Generate a white noise series.

        :param mu: The mu of the gaussian distribution.
        :param sigma: The sigma of the gaussian distribution.
        :param length: The number of elements in the returned series (> 0).
        :return: The white noise series.
        """
        return np.array([gauss(mu, sigma) for _i in range(length)], dtype=np.float64)

    @staticmethod
    def random_walk(
            volatility: float=0.1,
            delta_t: float=1 / (365 * 24 * 60),
            length: int=60 * 24 * 15) -> np.array:
        """
        Generate a random walk series.

        :param volatility: The volatility for the series (>= 0).
        :param delta_t: The unit of time (> 0).
        :param length: The number of elements in the returned series (> 0).
        :return: The random walk series.
        """
        return np.exp(np.random.normal(0, volatility, size=length) * np.sqrt(delta_t)).cumprod()


class BatterySeries:

    @staticmethod
    def create(length=4000) -> np.array:
        """
        Create a 'battery series'.

        Series comprises of a horizontally asymptotic
        dive, then a spike, then a step, the a sine wave and finally a high
        volatility random walk.

        :return: The battery series.
        """
        horizontal_asymptote = SeriesGenerator.horizontal_asymptote(initial=1.0)
        spike_function = SeriesGenerator.spike_function(count_post=1000)
        step_function = SeriesGenerator.step_function()
        sine_wave = SeriesGenerator.sine_wave()
        random_walk = SeriesGenerator.random_walk(volatility=20.0, length=length)

        battery_series = np.append(horizontal_asymptote, spike_function)
        battery_series = np.append(battery_series, step_function)
        battery_series = np.append(battery_series, sine_wave)
        battery_series = np.append(battery_series, random_walk)

        return battery_series

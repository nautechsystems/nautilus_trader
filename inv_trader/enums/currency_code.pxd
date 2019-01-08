#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="currency_code.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False


cpdef enum CurrencyCode:
    AUD = 36,
    CAD = 124,
    CHF = 756,
    CNY = 156,
    CNH = 999,
    CZK = 203,
    EUR = 978,
    GBP = 826,
    HKD = 344,
    JPY = 392,
    MXN = 484,
    NOK = 578,
    NZD = 554,
    SEK = 752,
    TRY = 949,
    SGD = 702,
    USD = 840,
    XAG = 961,
    XPT = 962,
    XAU = 959,
    ZAR = 710,
    UNKNOWN = -1

cdef inline str currency_code_string(int value):
    if value == 36:
        return "AUD"
    elif value == 124:
        return "CAD"
    elif value == 756:
        return "CHF"
    elif value == 156:
        return "CNY"
    elif value == 999:
        return "CNH"
    elif value == 203:
        return "CZK"
    elif value == 978:
        return "EUR"
    elif value == 826:
        return "GBP"
    elif value == 344:
        return "HKD"
    elif value == 392:
        return "JPY"
    elif value == 484:
        return "MXN"
    elif value == 578:
        return "NOK"
    elif value == 554:
        return "NZD"
    elif value == 752:
        return "SEK"
    elif value == 949:
        return "TRY"
    elif value == 702:
        return "SGD"
    elif value == 840:
        return "USD"
    elif value == 961:
        return "XAG"
    elif value == 962:
        return "XPT"
    elif value == 959:
        return "XAU"
    elif value == 710:
        return "ZAR"
    elif value == -1:
        return "UNKNOWN"
    else:
        return "UNKNOWN"

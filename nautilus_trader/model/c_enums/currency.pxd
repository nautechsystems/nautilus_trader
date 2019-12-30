# -------------------------------------------------------------------------------------------------
# <copyright file="currency.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cpdef enum Currency:
    UNKNOWN = -1,
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
    RUB = 643,
    SEK = 752,
    TRY = 949,
    SGD = 702,
    USD = 840,
    XAG = 961,
    XPT = 962,
    XAU = 959,
    ZAR = 710


cdef inline str currency_to_string(int value):
    if value == 36:
        return 'AUD'
    elif value == 124:
        return 'CAD'
    elif value == 756:
        return 'CHF'
    elif value == 156:
        return 'CNY'
    elif value == 999:
        return 'CNH'
    elif value == 203:
        return 'CZK'
    elif value == 978:
        return 'EUR'
    elif value == 826:
        return 'GBP'
    elif value == 344:
        return 'HKD'
    elif value == 392:
        return 'JPY'
    elif value == 484:
        return 'MXN'
    elif value == 578:
        return 'NOK'
    elif value == 554:
        return 'NZD'
    elif value == 643:
        return 'RUB'
    elif value == 752:
        return 'SEK'
    elif value == 949:
        return 'TRY'
    elif value == 702:
        return 'SGD'
    elif value == 840:
        return 'USD'
    elif value == 961:
        return 'XAG'
    elif value == 962:
        return 'XPT'
    elif value == 959:
        return 'XAU'
    elif value == 710:
        return 'ZAR'
    else:
        return 'UNKNOWN'


cdef inline Currency currency_from_string(str value):
    if value == 'AUD':
        return Currency.AUD
    elif value == 'CAD':
        return Currency.CAD
    elif value == 'CHF':
        return Currency.CHF
    elif value == 'CNY':
        return Currency.CNY
    elif value == 'CNH':
        return Currency.CNH
    elif value == 'CZK':
        return Currency.CZK
    elif value == 'EUR':
        return Currency.EUR
    elif value == 'GBP':
        return Currency.GBP
    elif value == 'HKD':
        return Currency.HKD
    elif value == 'JPY':
        return Currency.JPY
    elif value == 'MXN':
        return Currency.MXN
    elif value == 'NOK':
        return Currency.NOK
    elif value == 'NZD':
        return Currency.NZD
    elif value == 'RUB':
        return Currency.RUB
    elif value == 'SEK':
        return Currency.SEK
    elif value == 'TRY':
        return Currency.TRY
    elif value == 'SGD':
        return Currency.SGD
    elif value == 'USD':
        return Currency.USD
    elif value == 'XAG':
        return Currency.XAG
    elif value == 'XPT':
        return Currency.XPT
    elif value == 'XAU':
        return Currency.XAU
    elif value == 'ZAR':
        return Currency.ZAR
    else:
        return Currency.UNKNOWN

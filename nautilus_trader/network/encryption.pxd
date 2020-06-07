# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------


cdef class EncryptionSettings:
    cdef readonly bint use_encryption
    cdef readonly str algorithm
    cdef readonly str keys_dir

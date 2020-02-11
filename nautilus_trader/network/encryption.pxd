# -------------------------------------------------------------------------------------------------
# <copyright file="encryption.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cdef class EncryptionConfig:
    cdef readonly bint use_encryption
    cdef readonly str encryption_type
    cdef readonly str keys_dir

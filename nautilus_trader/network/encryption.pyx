# -------------------------------------------------------------------------------------------------
# <copyright file="encryption.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.correctness cimport Condition


cdef class EncryptionConfig:
    """
    Provides an encryption configuration.
    """

    def __init__(self,
                 bint use_encryption=False,
                 str encryption_type not None='',
                 str keys_dir not None=''):
        """
        Initializes a new instance of the EncryptionConfig class.

        :param use_encryption: If encryption should be used.
        :param encryption_type: The encryption type to be used.
        :param keys_dir: The path to the key certificates directory.
        """
        if use_encryption:
            Condition.valid_string(encryption_type, 'encryption_type')
            Condition.valid_string(keys_dir, 'key_dir')

        self.use_encryption = use_encryption
        self.encryption_type = encryption_type
        self.keys_dir = keys_dir

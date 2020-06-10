# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.correctness cimport Condition


cdef class EncryptionSettings:
    """
    Provides encryption settings.
    """

    def __init__(self, str algorithm not None='none', str keys_dir not None=''):
        """
        Initializes a new instance of the EncryptionConfig class.

        :param algorithm: The cryptographic algorithm type to be used.
        :param keys_dir: The path to the key certificates directory.
        """
        if algorithm == '':
            algorithm = 'none'
        use_encryption = algorithm != 'none'
        if use_encryption:
            Condition.valid_string(algorithm, 'algorithm')
            Condition.valid_string(keys_dir, 'key_dir')

        self.use_encryption = use_encryption
        self.algorithm = algorithm
        self.keys_dir = keys_dir

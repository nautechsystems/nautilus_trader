#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="linter.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import zmq.auth
from pathlib import Path

if __name__ == "__main__":

    keys_dir = 'path/to/your/keys'
    Path(keys_dir).mkdir(parents=True, exist_ok=True)

    zmq.auth.create_certificates(keys_dir, "client")

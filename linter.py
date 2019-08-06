#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="linter.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import os
import re

from typing import List

from setup_tools import scan_directories


def check_file_headers(directories: List[str], ignore: List[str], author: str) -> None:
    # Check file headers
    files = scan_directories(directories)
    checked_extensions = set()
    for file in files:
        if os.path.isfile(file):
            file_extension = os.path.splitext(file)[1]
            if file_extension not in ignore:
                checked_extensions.add(os.path.splitext(file)[1])
                with open(file, 'r') as open_file:
                    source_code = (open_file.read())
                    expected_file_name = file.split('/')[-1]
                    result = re.findall(r'\"(.+?)\"', source_code)
                    file_name = result[0]
                    company = result[1]
                    if file_name != expected_file_name:
                        raise ValueError(f"The file header for {file} is incorrect"
                                         f" (file= should be '{expected_file_name}' was '{file_name}')")
                    if company != author:
                        raise ValueError(f"The file header for {file} is incorrect"
                                         f" (company= should be '{author}' was '{company}')")

    print(f"Checked headers for extensions; {checked_extensions} file name and company name all OK")

#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="linter.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import os
import re

from typing import List

from tools.packaging import scan_directories


def check_file_headers(directories: List[str], to_lint: List[str], company_name: str) -> None:
    """
    Check the headers of all specified files for the following.

    - File name in header matches the actual file name.
    - Company name in header matches the given company name.

    :param directories: The list of directories for files to check.
    :param to_lint: The list of file extensions to lint.
    :param company_name: The expected company name.
    """
    files = scan_directories(directories)
    for file in files:
        if os.path.isfile(file):
            file_extension = os.path.splitext(file)[1]
            if file_extension in to_lint:
                with open(file, 'r') as open_file:
                    source_code = (open_file.read())
                    if source_code.startswith('# !linter_ignore'):
                        continue
                    expected_file_name = file.split('/')[-1]
                    result = re.findall(r'\"(.+?)\"', source_code)
                    if not result:
                        raise ValueError(f"No file header found in {file}.")
                    parsed_file_name = result[0]
                    parsed_company_name = result[1]
                    if parsed_file_name != expected_file_name:
                        raise ValueError(f"The file header for {file} is incorrect"
                                         f" (file= should be '{expected_file_name}' was '{parsed_file_name}').")
                    if parsed_company_name != company_name:
                        raise ValueError(f"The file header for {file} is incorrect"
                                         f" (company= should be '{company_name}' was '{parsed_company_name}').")

    print(f"Checked headers for extensions {to_lint}; The file name and company name are all OK.")


def check_docstrings(directories: List[str], to_lint: List[str]):
    """
    Check the headers of all specified files for the following.

    - File name in header matches the actual file name.
    - Company name in header matches the given company name.

    :param directories: The list of directories for files to check.
    :param to_lint: The list of file extensions to lint.
    """
    pass
    # files = scan_directories(directories)
    # for file in files:
    #     if os.path.isfile(file):
    #         file_extension = os.path.splitext(file)[1]
    #         if file_extension in to_lint:
    #             with open(file, 'r') as open_file:
    #                 x = source_code = (open_file.read())

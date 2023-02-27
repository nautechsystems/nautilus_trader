# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import pytest


def pytest_collection_modifyitems(config, items):
    """Skip any tests that exist on the base classes, while allowing them to run in their subclasses."""
    from tests.integration_tests.adapters.base.base_data import TestBaseDataClient
    from tests.integration_tests.adapters.base.base_execution import TestBaseExecClient

    TEMPLATE_CLASSES = (TestBaseExecClient, TestBaseDataClient)

    for item in items:
        if item.cls in TEMPLATE_CLASSES:
            item.add_marker(pytest.mark.skip(reason="base_class"))

import pytest


def pytest_collection_modifyitems(config, items):
    """Skip any tests that exist on the base classes, while allowing them to run in their subclasses."""
    from tests.integration_tests.adapters._template.test_template_data import TestBaseDataClient
    from tests.integration_tests.adapters._template.test_template_execution import (
        TestBaseExecClient,
    )

    TEMPLATE_CLASSES = (TestBaseExecClient, TestBaseDataClient)
    for item in items:
        if item.cls in TEMPLATE_CLASSES:
            item.add_marker(pytest.mark.skip(reason="template"))

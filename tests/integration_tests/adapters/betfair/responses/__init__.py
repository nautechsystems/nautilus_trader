import json

from tests import TESTS_PACKAGE_ROOT


TEST_PATH = TESTS_PACKAGE_ROOT + "/integration_tests/adapters/ccxt/responses/"


def load_json(name):
    return json.loads(open(TEST_PATH + name).read())

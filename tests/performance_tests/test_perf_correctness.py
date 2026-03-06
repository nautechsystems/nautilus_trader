from nautilus_trader.core.correctness import PyCondition


def test_condition_none(benchmark):
    benchmark(PyCondition.none, None, "param")


def test_condition_true(benchmark):
    benchmark(PyCondition.is_true, True, "this should be true")


def test_condition_valid_string(benchmark):
    benchmark(PyCondition.valid_string, "abc123", "string_param")


def test_condition_type_or_none(benchmark):
    benchmark(PyCondition.type_or_none, "hello", str, "world")

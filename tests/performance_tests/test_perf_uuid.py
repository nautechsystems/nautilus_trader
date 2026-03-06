import uuid

from nautilus_trader.core.uuid import UUID4


def test_make_builtin_uuid(benchmark):
    benchmark(uuid.uuid4)


def test_make_nautilus_uuid(benchmark):
    benchmark(UUID4)


def test_nautilus_uuid_value(benchmark):
    uuid = UUID4()

    benchmark(lambda: uuid.value)


def test_nautilus_uuid_from_value(benchmark):
    uuid = UUID4()
    value = uuid.value

    benchmark(lambda: UUID4.from_str(value))

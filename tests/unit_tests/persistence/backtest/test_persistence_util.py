from nautilus_trader.persistence.util import Singleton
from nautilus_trader.persistence.util import clear_singleton_instances
from nautilus_trader.persistence.util import resolve_kwargs


def test_resolve_kwargs():
    def func1():
        pass

    def func2(a, b, c):
        pass

    assert resolve_kwargs(func1) == {}
    assert resolve_kwargs(func2, 1, 2, 3) == {"a": 1, "b": 2, "c": 3}
    assert resolve_kwargs(func2, 1, 2, c=3) == {"a": 1, "b": 2, "c": 3}
    assert resolve_kwargs(func2, 1, c=3, b=2) == {"a": 1, "b": 2, "c": 3}
    assert resolve_kwargs(func2, a=1, b=2, c=3) == {"a": 1, "b": 2, "c": 3}


def test_singleton_without_init():
    # Arrange
    class Test(metaclass=Singleton):
        pass

    # Arrange
    test1 = Test()
    test2 = Test()

    # Assert
    assert test1 is test2


def test_singleton_with_init():
    # Arrange
    class Test(metaclass=Singleton):
        def __init__(self, a, b):
            self.a = a
            self.b = b

    # Act
    test1 = Test(1, 1)
    test2 = Test(1, 1)
    test3 = Test(1, 2)

    # Assert
    assert test1 is test2
    assert test2 is not test3


def test_clear_instance():
    # Arrange
    class Test(metaclass=Singleton):
        pass

    # Act
    Test()
    assert Test._instances

    clear_singleton_instances(Test)

    assert not Test._instances

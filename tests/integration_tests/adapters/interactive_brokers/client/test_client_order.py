from unittest.mock import MagicMock

from ibapi.order import Order as IBOrder


def test_place_order(ib_client):
    # Arrange
    ib_order = IBOrder()
    ib_order.orderId = 1
    ib_order.contract = MagicMock()
    ib_client._eclient.placeOrder = MagicMock()

    # Act
    ib_client.place_order(ib_order)

    # Assert
    ib_client._eclient.placeOrder.assert_called_with(
        ib_order.orderId,
        ib_order.contract,
        ib_order,
    )


def test_place_order_list(ib_client):
    # Arrange

    # Act

    # Assert
    pass


def test_cancel_order(ib_client):
    # Arrange

    # Act

    # Assert
    pass


def test_cancel_all_orders(ib_client):
    # Arrange

    # Act

    # Assert
    pass


def test_get_open_orders(ib_client):
    # Arrange

    # Act

    # Assert
    pass


def test_next_order_id(ib_client):
    # Arrange

    # Act

    # Assert
    pass


def test_process_order_status(ib_client):
    # Arrange

    # Act

    # Assert
    pass

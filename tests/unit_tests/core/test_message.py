import pickle

from nautilus_trader.core.message import Command
from nautilus_trader.core.message import Document
from nautilus_trader.core.message import Request
from nautilus_trader.core.message import Response
from nautilus_trader.core.uuid import UUID4


class TestMessage:
    def test_command_message_picking(self):
        # Arrange
        command = Command(
            UUID4(),
            0,
        )

        # Act
        pickled = pickle.dumps(command)
        unpickled = pickle.loads(pickled)  # noqa: S301 (pickle is safe here)

        # Assert
        assert command == unpickled

    def test_document_message_picking(self):
        # Arrange
        doc = Document(
            UUID4(),
            0,
        )

        # Act
        pickled = pickle.dumps(doc)
        unpickled = pickle.loads(pickled)  # noqa: S301 (pickle is safe here)

        # Assert
        assert doc == unpickled

    def test_request_message_pickling(self):
        # Arrange
        req = Request(
            print,
            UUID4(),
            0,
        )

        # Act
        pickled = pickle.dumps(req)
        unpickled = pickle.loads(pickled)  # noqa: S301 (pickle is safe here)

        # Assert
        assert req == unpickled

    def test_response_message_pickling(self):
        # Arrange
        res = Response(
            UUID4(),
            UUID4(),
            0,
        )

        # Act
        pickled = pickle.dumps(res)
        unpickled = pickle.loads(pickled)  # noqa: S301 (pickle is safe here)

        # Assert
        assert res == unpickled

    def test_document_message_hash(self):
        # Arrange
        message = Document(
            document_id=UUID4(),
            ts_init=0,
        )

        # Act, Assert
        assert isinstance(hash(message), int)

    def test_document_message_str_and_repr(self):
        # Arrange
        uuid = UUID4()
        message = Document(
            document_id=uuid,
            ts_init=0,
        )

        # Act, Assert
        assert str(message) == f"Document(id={uuid}, ts_init=0)"
        assert str(message) == f"Document(id={uuid}, ts_init=0)"

    def test_response_message_str_and_repr(self):
        # Arrange
        uuid_id = UUID4()
        uuid_corr = UUID4()
        response = Response(
            correlation_id=uuid_corr,
            response_id=uuid_id,
            ts_init=0,
        )

        # Act, Assert
        assert str(response) == f"Response(correlation_id={uuid_corr}, id={uuid_id}, ts_init=0)"
        assert str(response) == f"Response(correlation_id={uuid_corr}, id={uuid_id}, ts_init=0)"

import pytest

from nautilus_trader.data.socket import SocketClient


@pytest.mark.asyncio
async def test_socket_base(socket_server, event_loop):
    messages = []

    def handler(raw):
        messages.append(raw)
        if len(messages) > 5:
            client.stop = True

    host, port = socket_server.server_address
    client = SocketClient(
        host=host,
        port=port,
        message_handler=handler,
        loop=event_loop,
        ssl=False,
    )
    await client.start()
    assert messages == [b"hello"] * 6

import asyncio

from nautilus_trader.core.nautilus_pyo3.network import HttpClient
from nautilus_trader.core.nautilus_pyo3.network import HttpResponse


async def make_request(client: HttpClient) -> HttpResponse:
    return await client.request("get", "https://github.com", {})


# TODO: need to install pytest-asyncio to handle async tests using pytest
if __name__ == "__main__":
    client = HttpClient()
    res = asyncio.run(make_request(client))
    assert res.status == 200
    assert len(res.body) != 0

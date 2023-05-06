import asyncio

from nautilus_network import HttpClient


async def make_request(client: HttpClient):
    return await client.request("get", "https://github.com", {})


# TODO: need to install pytest-asyncio to introduce async tests
if __name__ == "__main__":
    client = HttpClient()
    res = asyncio.run(make_request(client))
    assert res.status == 200
    assert len(res.body) != 0

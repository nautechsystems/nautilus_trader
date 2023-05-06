import asyncio

from nautilus_network import HttpClient


async def make_request(client: HttpClient):
    return await client.request("get", "https://github.com", {})


if __name__ == "__main__":
    client = HttpClient()
    body = asyncio.run(make_request(client))
    assert len(body) != 0

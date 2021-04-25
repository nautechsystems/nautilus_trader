import asyncio
import itertools
from typing import Optional

import aiohttp
import socket

"""
TODO:
    add logger
    add clock(set_timer)
"""

cdef callable select_serializer(str url):
    import gzip
    import zlib
    try:
        import orjson as json
    except ImportError:
        import json

    if url in [
        'wss://api.huobi.pro/ws',               # Huobi Spot
        'wss://api.hbdm.com/ws',                # Huobi Futures
        'wss://api.hbdm.com/swap-notification'  # Huobi Swap
    ]:
        return lambda data: json.loads(gzip.decompress(data).decode('utf-8'))
    elif url in [
        'wss://real.okex.com:8443/ws/v3'        # Okex Spot, Futures, Swap
    ]:
        return lambda data: json.loads(zlib.decompress(data, wbits=-zlib.MAX_WBITS))
    else:
        return json.loads


cdef class HttpClient:
    pass

cdef class WebsocketClient(HttpClient):
    def __init__(
        self,
        list addresses=None,
        list nameservers=None,
        int ttl_dns_cache=3600,
        int connection_timeout=60
    ):
        self._addresses = addresses or ['0.0.0.0']
        self._nameservers = nameservers or ['8.8.8.8', '8.8.4.4']
        self._is_connected = False

        self._sessions = itertools.cycle([aiohttp.ClientSession(
            connector=aiohttp.TCPConnector(
                limit=0,
                resolver=aiohttp.AsyncResolver(
                    nameservers=self._nameservers
                ),
                local_addr=(address, 0),
                ttl_dns_cache=3600,
                faily=socket.AF_INET,
                ssl=False
            )) for address in self._addresses
        ])

    def connection(
        self,
        url: str,
        on_open: callable,
        on_message: callable,
        on_ping: Optional[callable] = None,
        on_pong: Optional[callable] = None,
        timeout: float = 5.0,
        receive_timeout: Optional[float] = None,
        reconnect_interval: float = 30
    ):


        cdef :
            callable serializer = select_serializer(url)
            aiohttp.WSMessage msg
            dict serialized_msg

        return

        try:
            async with next(self._sessions).ws_connect(
                url=url,
                timeout=timeout,
                receive_timeout=receive_timeout
            ) as ws:
                await on_open(ws)
                async for msg in ws:
                    serialized_msg = serializer(msg.data)

                    if msg.type == aiohttp.WSMsgType.TEXT:
                        await on_message(ws, serialized_msg)
                    if msg.type == aiohttp.WSMsgType.BINARY:
                        print("WSMsgType.BINARY")
                        await on_message(ws, serialized_msg)
                    elif msg.type == aiohttp.WSMsgType.PING and \
                        on_ping is not None:
                        await on_ping(ws, serialized_msg)
                    elif msg.type == aiohttp.WSMsgType.PONG and \
                             on_pong is not None:
                        await on_pong(ws, serialized_msg)
                    elif msg.type == aiohttp.WSMsgType.ERROR:
                        print("ERROR")
                    elif msg.type == aiohttp.WSMsgType.CLOSED:
                        print("CLOSED")
                    elif msg.type == aiohttp.WSMsgType.CLOSE:
                        print("CLOSE")
                    elif msg.type == aiohttp.WSMsgType.CLOSING:
                        print("CLOSING")
        except Exception as e:
            await asyncio.sleep(reconnect_interval)





# flake8: noqa

# ******************************************************************************
# The following is experimental code which will not be included in a stable
# release. There has been various modifications to the original code.
# ******************************************************************************

'''
Copyright (C) 2017-2021  Bryant Moscon - bmoscon@gmail.com
 Permission is hereby granted, free of charge, to any person obtaining a copy
 of this software and associated documentation files (the "Software"), to
 deal in the Software without restriction, including without limitation the
 rights to use, copy, modify, merge, publish, distribute, sublicense, and/or
 sell copies of the Software, and to permit persons to whom the Software is
 furnished to do so, subject to the following conditions:
 1. Redistributions of source code must retain the above copyright notice,
    this list of conditions, and the following disclaimer.
 2. Redistributions in binary form must reproduce the above copyright notice,
    this list of conditions and the following disclaimer in the documentation
    and/or other materials provided with the distribution, and in the same
    place and form as other copyright, license and disclaimer information.
 3. The end-user documentation included with the redistribution, if any, must
    include the following acknowledgment: "This product includes software
    developed by Bryant Moscon (http://www.bryantmoscon.com/)", in the same
    place and form as other third-party acknowledgments. Alternately, this
    acknowledgment may appear in the software itself, in the same form and
    location as other such third-party acknowledgments.
 4. Except as contained in this notice, the name of the author, Bryant Moscon,
    shall not be used in advertising or otherwise to promote the sale, use or
    other dealings in this Software without prior written authorization from
    the author.
 THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
 THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
 THE SOFTWARE.
'''

import asyncio
from collections import defaultdict
import functools
import logging
from signal import SIGABRT
from signal import SIGHUP
from signal import SIGINT
from signal import SIGTERM
from socket import error as socket_error
from time import time
import zlib

from cryptofeed.config import Config
from cryptofeed.defines import BINANCE
from cryptofeed.defines import BINANCE_DELIVERY
from cryptofeed.defines import BINANCE_FUTURES
from cryptofeed.defines import BINANCE_US
from cryptofeed.defines import BITCOINCOM
from cryptofeed.defines import BITFINEX
from cryptofeed.defines import BITFLYER
from cryptofeed.defines import BITMAX
from cryptofeed.defines import BITMEX
from cryptofeed.defines import BITSTAMP
from cryptofeed.defines import BITTREX
from cryptofeed.defines import BLOCKCHAIN
from cryptofeed.defines import BYBIT
from cryptofeed.defines import COINBASE
from cryptofeed.defines import COINGECKO
from cryptofeed.defines import DERIBIT
from cryptofeed.defines import EXX as EXX_str
from cryptofeed.defines import FTX as FTX_str
from cryptofeed.defines import FTX_US
from cryptofeed.defines import GATEIO
from cryptofeed.defines import GEMINI
from cryptofeed.defines import HITBTC
from cryptofeed.defines import HUOBI
from cryptofeed.defines import HUOBI_DM
from cryptofeed.defines import HUOBI_SWAP
from cryptofeed.defines import KRAKEN
from cryptofeed.defines import KRAKEN_FUTURES
from cryptofeed.defines import L2_BOOK
from cryptofeed.defines import OKCOIN
from cryptofeed.defines import OKEX
from cryptofeed.defines import POLONIEX
from cryptofeed.defines import PROBIT
from cryptofeed.defines import UPBIT
from cryptofeed.defines import WHALE_ALERT
from cryptofeed.exceptions import ExhaustedRetries
from cryptofeed.exchanges import *
from cryptofeed.log import get_logger
from cryptofeed.nbbo import NBBO
from cryptofeed.providers import *
from websockets import ConnectionClosed


# LOG = logging.getLogger('feedhandler')


# Maps string name to class name for use with config
_EXCHANGES = {
    BINANCE: Binance,
    BINANCE_US: BinanceUS,
    BINANCE_FUTURES: BinanceFutures,
    BINANCE_DELIVERY: BinanceDelivery,
    BITCOINCOM: BitcoinCom,
    BITFINEX: Bitfinex,
    BITFLYER: Bitflyer,
    BITMAX: Bitmax,
    BITMEX: Bitmex,
    BITSTAMP: Bitstamp,
    BITTREX: Bittrex,
    BLOCKCHAIN: Blockchain,
    BYBIT: Bybit,
    COINBASE: Coinbase,
    COINGECKO: Coingecko,
    DERIBIT: Deribit,
    EXX_str: EXX,
    FTX_str: FTX,
    FTX_US: FTXUS,
    GEMINI: Gemini,
    HITBTC: HitBTC,
    HUOBI_DM: HuobiDM,
    HUOBI_SWAP: HuobiSwap,
    HUOBI: Huobi,
    KRAKEN_FUTURES: KrakenFutures,
    KRAKEN: Kraken,
    OKCOIN: OKCoin,
    OKEX: OKEx,
    POLONIEX: Poloniex,
    UPBIT: Upbit,
    GATEIO: Gateio,
    PROBIT: Probit,
    WHALE_ALERT: WhaleAlert
}


def setup_signal_handlers(loop):
    """
    This must be run from the loop in the main thread
    """
    def handle_stop_signals():
        raise SystemExit

    for signal in (SIGTERM, SIGINT, SIGHUP, SIGABRT):
        loop.add_signal_handler(signal, handle_stop_signals)


class FeedHandler:
    def __init__(self, retries=10, timeout_interval=10, log_messages_on_error=False, raw_message_capture=None, handler_enabled=True, config=None):
        """
        retries: int
            number of times the connection will be retried (in the event of a disconnect or other failure)
        timeout_interval: int
            number of seconds between checks to see if a feed has timed out
        log_messages_on_error: boolean
            if true, log the message from the exchange on exceptions
        raw_message_capture: callback
            if defined, callback to save/process/handle raw message (primarily for debugging purposes)
        handler_enabled: boolean
            run message handlers (and any registered callbacks) when raw message capture is enabled
        config: str
            absolute path (including file name) of the config file. If not provided env var checked first, then local config.yaml
        """
        self.feeds = []
        self.retries = retries
        self.timeout = {}
        self.last_msg = defaultdict(lambda: None)
        self.timeout_interval = timeout_interval
        self.log_messages_on_error = log_messages_on_error
        self.raw_message_capture = raw_message_capture
        self.handler_enabled = handler_enabled
        self.config = Config(file_name=config)

        # lfile = 'feedhandler.log' if not self.config or not self.config.log.filename else self.config.log.filename
        # level = logging.WARNING if not self.config or not self.config.log.level else self.config.log.level
        # get_logger('feedhandler', lfile, level)

    def playback(self, feed, filenames):
        loop = asyncio.get_event_loop()
        return loop.run_until_complete(self._playback(feed, filenames))

    async def _playback(self, feed, filenames):
        counter = 0
        callbacks = defaultdict(int)

        class FakeWS:
            async def send(self, *args, **kwargs):
                pass

        async def internal_cb(*args, **kwargs):
            callbacks[kwargs['cb_type']] += 1

        for cb_type, handler in feed.callbacks.items():
            f = functools.partial(internal_cb, cb_type=cb_type)
            handler.append(f)

        await feed.subscribe(FakeWS())

        for filename in filenames if isinstance(filenames, list) else [filenames]:
            with open(filename, 'r') as fp:
                for line in fp:
                    timestamp, message = line.split(":", 1)
                    counter += 1
                    await feed.message_handler(message, timestamp)
            return {'messages_processed': counter, 'callbacks': dict(callbacks)}

    def add_feed(self, feed, timeout=120, **kwargs):
        """
        feed: str or class
            the feed (exchange) to add to the handler
        timeout: int
            number of seconds without a message before the feed is considered
            to be timed out. The connection will be closed, and if retries
            have not been exhausted, the connection will be restablished.
            If set to -1, no timeout will occur.
        kwargs: dict
            if a string is used for the feed, kwargs will be passed to the
            newly instantiated object
        """
        if isinstance(feed, str):
            if feed in _EXCHANGES:
                self.feeds.append((_EXCHANGES[feed](**kwargs), timeout))
            else:
                raise ValueError("Invalid feed specified")
        else:
            self.feeds.append((feed, timeout))

    def add_nbbo(self, feeds, pairs, callback, timeout=120):
        """
        feeds: list of feed classes
            list of feeds (exchanges) that comprises the NBBO
        pairs: list str
            the trading pairs
        callback: function pointer
            the callback to be invoked when a new tick is calculated for the NBBO
        timeout: int
            seconds without a message before a connection will be considered dead and reestablished.
            See `add_feed`
        """
        cb = NBBO(callback, pairs)
        for feed in feeds:
            self.add_feed(feed(channels=[L2_BOOK], pairs=pairs, callbacks={L2_BOOK: cb}), timeout=timeout)

    def run_feed(self, loop, feed, timeout=120):
        for conn, sub, handler in feed.connect():
            loop.create_task(self._connect(conn, sub, handler))
            self.timeout[conn.uuid] = timeout

    def run(self, loop, start_loop: bool = True, install_signal_handlers: bool = True):
        """
        start_loop: bool, default True
            if false, will not start the event loop. Also, will not
            use uvlib/uvloop if false, the caller will
            need to init uvloop if desired.
        install_signal_handlers: bool, default True
            if True, will install the signal handlers on the event loop. This
            can only be done from the main thread's loop, so if running cryptofeed on
            a child thread, this must be set to false, and setup_signal_handlers must
            be called from the main/parent thread's event loop
        """
        if len(self.feeds) == 0:
            # LOG.error('No feeds specified')
            raise ValueError("No feeds specified")

        try:
            if start_loop:
                try:
                    import uvloop
                    asyncio.set_event_loop_policy(uvloop.EventLoopPolicy())
                except ImportError:
                    pass

            loop = asyncio.get_event_loop()
            # Good to enable when debugging
            # loop.set_debug(True)

            if install_signal_handlers:
                setup_signal_handlers(loop)

            for feed, timeout in self.feeds:
                for conn, sub, handler in feed.connect():
                    loop.create_task(self._connect(conn, sub, handler))
                    self.timeout[conn.uuid] = timeout

            if start_loop:
                loop.run_forever()

        except SystemExit:
            pass
            # LOG.info("System Exit received - shutting down")
        except Exception:
            pass
            # LOG.error("Unhandled exception", exc_info=True)
        finally:
            for feed, _ in self.feeds:
                loop.run_until_complete(feed.stop())

    async def _watch(self, connection):
        if self.timeout[connection.uuid] == -1:
            return

        while connection.open:
            if self.last_msg[connection.uuid]:
                if time() - self.last_msg[connection.uuid] > self.timeout[connection.uuid]:
                    # LOG.warning("%s: received no messages within timeout, restarting connection", connection.uuid)
                    await connection.close()
                    break
            await asyncio.sleep(self.timeout_interval)

    async def _connect(self, conn, subscribe, handler):
        """
        Connect to exchange and subscribe
        """
        retries = 0
        delay = 1  # conn.delay
        while retries <= self.retries or self.retries == -1:
            self.last_msg[conn.uuid] = None
            try:
                async with conn.connect() as connection:
                    asyncio.ensure_future(self._watch(connection))
                    # connection was successful, reset retry count and delay
                    retries = 0
                    delay = 1  # conn.delay
                    await subscribe(connection)
                    await self._handler(connection, handler)
            except (ConnectionClosed, ConnectionAbortedError, ConnectionResetError, socket_error) as e:
                # LOG.warning("%s: encountered connection issue %s - reconnecting...", conn.uuid, str(e), exc_info=True)
                await asyncio.sleep(delay)
                retries += 1
                delay *= 2
            except Exception:
                # LOG.error("%s: encountered an exception, reconnecting", conn.uuid, exc_info=True)
                await asyncio.sleep(delay)
                retries += 1
                delay *= 2

        # LOG.error("%s: failed to reconnect after %d retries - exiting", conn.uuid, retries)
        raise ExhaustedRetries()

    async def _handler(self, connection, handler):
        try:
            if self.raw_message_capture and self.handler_enabled:
                async for message in connection.read():
                    self.last_msg[connection.uuid] = time()
                    await self.raw_message_capture(message, self.last_msg[connection.uuid], connection.uuid)
                    await handler(message, connection, self.last_msg[connection.uuid])
            elif self.raw_message_capture:
                async for message in connection.read():
                    self.last_msg[connection.uuid] = time()
                    await self.raw_message_capture(message, self.last_msg[connection.uuid], connection.uuid)
            else:
                async for message in connection.read():
                    self.last_msg[connection.uuid] = time()
                    await handler(message, connection, self.last_msg[connection.uuid])
        except Exception:
            if self.log_messages_on_error:
                if connection.uuid in {HUOBI, HUOBI_DM}:
                    message = zlib.decompress(message, 16 + zlib.MAX_WBITS)
                elif connection.uuid in {OKCOIN, OKEX}:
                    message = zlib.decompress(message, -15)
                # LOG.error("%s: error handling message %s", connection.uuid, message)
            # exception will be logged with traceback when connection handler
            # retries the connection
            raise

import json

import ccxt


exchange = 'bitfinex'
ccxt = getattr(ccxt, exchange.lower())()
ccxt.load_markets()

# precisions = [{k: v['precision']} for k, v in ccxt.markets.items()]
# print(json.dumps(precisions, sort_keys=True, indent=4))

# instruments = {k: v for k, v in ccxt.markets.items()}
# print(json.dumps(instruments, sort_keys=True, indent=4))

# currencies = {k: v for k, v in ccxt.currencies.items()}
# print(json.dumps(currencies, sort_keys=True, indent=4))

# currencies = {k: v for k, v in ccxt.currencies.items()}
# print(ccxt.fetch_ohlcv('BTC/USDT', '1m', limit=1))

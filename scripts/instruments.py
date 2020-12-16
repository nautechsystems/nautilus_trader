import json

import ccxt


exchange = 'binance'
ccxt = getattr(ccxt, exchange.lower())()
ccxt.load_markets()

# precisions = [{k: v['precision']} for k, v in ccxt.markets.items()]
# print(json.dumps(precisions, sort_keys=True, indent=4))

instruments = {k: v for k, v in ccxt.markets.items()}
print(json.dumps(instruments["BTC/USDT"], sort_keys=True, indent=4))

# currencies = {k: v for k, v in ccxt.currencies.items()}
# print(json.dumps(currencies, sort_keys=True, indent=4))

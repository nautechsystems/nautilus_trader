import json

import ccxt

exchange = 'bitmex'
ccxt = getattr(ccxt, exchange.lower())()
ccxt.load_markets()

precision = [{k: m['precision']} for k, m in ccxt.markets.items()]
print(json.dumps(precision, sort_keys=True, indent=4))

import json

import ccxt


exchange = 'binance'
ccxt = getattr(ccxt, exchange.lower())()
ccxt.load_markets()

# precisions = [{k: m['precision']} for k, m in ccxt.markets.items()]
# print(json.dumps(precisions, sort_keys=True, indent=4))

instruments = {k: m for k, m in ccxt.markets.items()}
print(json.dumps(instruments['ETHUSDT'], sort_keys=True, indent=4))

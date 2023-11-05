# Databento

NautilusTrader offers an adapter for integrating with the Databento API and [Databento Binary Encoding (DBN)](https://docs.databento.com/knowledge-base/new-users/dbn-encoding) format data.
This includes loading historical data from disk into Nautilus objects for research and backtesting purposes,
as well as subscribing to real-time data feeds to support live trading.

```{tip}
For testing purposes, [Databento](https://databento.com/signup) currently offers $125 USD in free data credits for new account sign-ups.
```

## Overview

The following adapter classes are available:
- `DatabentoDataLoader` which allows loading Databento Binary Encoding (DBN) data from disk.
- `DatabentoInstrumentProvider` which connects to the Databento API to provide latest or historical instrument definitions.
- `DatabentoDataClient` which allows requesting historical market data and subscribing to real-time data feeds.

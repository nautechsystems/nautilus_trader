# Integrations

NautilusTrader is designed to work with modular adapters which provide integrations 
with data publishers and/or trading venues (exchanges/brokers).

```{warning}
The initial integrations for the project are currently under heavy construction. 
It's advised to conduct some of your own testing with small amounts of capital before
running strategies which are able to access larger capital allocations.
```

The implementation of each integration aims to meet the following criteria:

- Low-level client components should match the exchange API as closely as possible.
- The full range of an exchanges functionality (where applicable to NautilusTrader), should _eventually_ be supported.
- Exchange specific data types will be added to support the functionality and return
  types which are reasonably expected by a user.
- Actions which are unsupported by either the exchange or NautilusTrader, will be explicitly logged as
a warning or error when a user attempts to perform said action.

## API Unification
All integrations must be compatible with the NautilusTrader API at the system boundary,
this means there is some unification and standardization needed.

- All symbols will match the native/local symbol for the exchange.
- All timestamps will be normalized to UNIX nanoseconds.

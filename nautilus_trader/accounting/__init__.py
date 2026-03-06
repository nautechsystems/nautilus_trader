"""
The `accounting` subpackage defines both different account types and account management
machinery.

There is also an `ExchangeRateCalculator` for calculating the exchange rate between FX and/or Crypto
pairs. The `AccountManager` is mainly used from the `Portfolio` to manage accounting operations.

The `AccountFactory` supports customized account types for specific integrations. These custom
account types can be registered with the factory and will then be instantiated when an `AccountState`
event is received for that integration.

"""

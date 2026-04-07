# Example - Adanos Market Sentiment Custom Data

This example shows how to move Adanos market sentiment into NautilusTrader as
`CustomData` and use it alongside equity bars in a backtest.

## What You'll Learn

- How to define a Nautilus-native custom data type for Adanos sentiment snapshots.
- How to wrap source rows into `CustomData` for routing metadata.
- How to subscribe to cross-source sentiment updates from a strategy.

## Why This Matters

Alternative data is most useful in NautilusTrader when it behaves like any other
first-class data stream. This example keeps the integration simple:

- build a normalized `AdanosSentimentSnapshot`
- route it through the standard `subscribe_data()` / `on_data()` flow
- persist the same snapshots in the Parquet catalog for research-to-live parity

The example uses synthetic AAPL bars and synthetic Adanos compare rows so it can
run without external API credentials.

## Run The Example

From the repository root:

```bash
PYTHONPATH=. .venv/bin/python examples/backtest/example_12_adanos_custom_data/run_example.py
```

You should see the strategy subscribe to the custom sentiment stream, log each
sentiment update, and print a simple conviction verdict alongside each daily
bar.

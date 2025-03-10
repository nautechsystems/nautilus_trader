# Portfolio Example

A simple strategy demonstrating how to use Portfolio in NautilusTrader.

The Portfolio is a central component that tracks the state of your trading account.
It connects directly to the broker to get real-time positions, balances, and P&L.

## Example Highlights

The strategy shows portfolio information at four key points:

1. **Initial State**: Before any trades are executed.
2. **Position Open**: When a new position is created.
3. **Mid-Trade**: Two minutes after position opening.
4. **Final State**: After all positions are closed (when strategy stops).

To simulate these specific portfolio states, the strategy fires bracket order (a combination of an entry order
with associated take-profit and stop-loss orders), allowing us to demonstrate the complete lifecycle of portfolio states.

## Additional info

Key differences between `Portfolio` and `Cache`:

`Portfolio`:

- Gets data directly from broker for maximum accuracy.
- Best for real-time position and risk management.
- Provides authoritative account state (margins, balances).
- Should be used for critical trading decisions.

`Cache`:

- Stores all trading data in system memory.
- Useful for quick access to historical data and market state.
- More efficient for frequent queries as it avoids broker round-trips.
- Updates automatically as new data arrives.
- Might have minimal delay compared to broker data.

## Additional Resources

For more information about Portfolio in NautilusTrader, see:

- Portfolio API documentation - search the codebase for `Portfolio` class.
- Portfolio concept guide - see the "Portfolio" section in the documentation for more details.

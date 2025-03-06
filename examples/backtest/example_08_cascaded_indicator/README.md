# Example: Using cascaded technical indicators

This example demonstrates how to use cascaded technical indicators in a **NautilusTrader** strategy.

The example shows how to set up and use two Exponential Moving Average (EMA) indicators in a cascaded manner,
where the second indicator (EMA-20) is calculated using values from the first indicator (EMA-10),
demonstrating proper initialization, updating, and accessing indicator values in a cascaded setup.

**What this example demonstrates:**

- Creating and configuring multiple technical indicators (EMAs).
- Setting up a cascaded indicator relationship.
- Registering the primary indicator to receive bar data.
- Manually updating the cascaded indicator.
- Storing and accessing historical values for both indicators.
- Proper handling of indicator initialization in a cascaded setup.

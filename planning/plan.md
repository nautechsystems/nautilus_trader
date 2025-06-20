# Polymarket Market Making Strategy Plan

## Strategy Plan

- User wants to create a strategy to market make the Polymarket market: "Will the Indiana Pacers win the 2025 NBA Finals?"
- The user provided the market link: <https://polymarket.com/event/nba-champion-2024-2025/will-the-indiana-pacers-win-the-2025-nba-finals?tid=1750440387421>
- Guidelines in AGENTS.md should be followed for this task.
- AGENTS.md guidelines: use `uv` for dependencies, follow PEP8, format code, run tests, write concise comments and docstrings, use imperative mood.
- Automated Polymarket market making involves two-sided liquidity, placing orders close to the current price, and maximizing liquidity rewards.
- Open-source bot (poly-maker) and liquidity rewards structure have been identified as key resources.

### Winning Strategy: Reward Maximization with Inventory Control

- Place buy and sell orders as close to the mid-price as possible to maximize rewards.
- Always maintain two-sided liquidity for the 3x reward multiplier.
- Dynamically adjust spread and order size to keep inventory balanced and avoid large directional exposure.
- Withdraw or rebalance if market moves sharply or inventory risk grows.
- Monitor reward pool and adapt to changes in Polymarket’s incentive structure.

---

## Implementation Plan

### 1. Project Setup

- Use `uv` for dependency management (per AGENTS.md).
- Set up a new Python project or module within your codebase.
- Initialize pre-commit hooks and formatters.

### 2. API Integration

- Integrate with Polymarket’s API to:
  - Fetch real-time order book and price data for the Indiana Pacers NBA Finals market.
  - Place/cancel buy and sell orders.
  - Monitor your positions and inventory.

### 3. Core Components

- Data Collector: Continuously pulls order book, trade, and reward pool data.
- Reward Calculator: Estimates expected liquidity rewards for various quoting positions.
- Inventory Manager: Tracks your current position and risk.
- Quote Engine: Determines optimal prices and sizes for buy/sell orders.
- Execution Engine: Places, updates, and cancels orders as needed.

### 4. Strategy Logic

- Place buy and sell orders as close to the mid-price as possible.
- Always maintain both sides of the book for maximum reward multiplier.
- Dynamically adjust spread and size to keep inventory balanced.
- Withdraw/rebalance if inventory risk grows or market moves sharply.
- Monitor and adapt to changes in reward pool or market volatility.

### 5. Monitoring & Logging

- Log all trades, order placements, cancellations, and inventory changes.
- Provide real-time dashboard or CLI output for live monitoring.

### 6. Testing & Simulation

- Unit tests for each component.
- Backtest strategy logic using historical market data.
- Simulate various market conditions and inventory scenarios.

### 7. Deployment

- Run the bot in paper trading mode first.
- Deploy live with small capital, gradually scaling up.

---

## Code Outline Plan

```
polymarket_maker/
│
├── __init__.py
├── config.py                # API keys, market IDs, parameters
├── main.py                  # Entry point, CLI
├── api/
│   ├── __init__.py
│   ├── polymarket_client.py # REST/websocket API integration
│
├── data/
│   ├── __init__.py
│   ├── collector.py         # Fetches market/order book data
│
├── strategy/
│   ├── __init__.py
│   ├── reward_calc.py       # Computes expected rewards
│   ├── inventory.py         # Inventory and risk management
│   ├── quote_engine.py      # Determines optimal quotes
│
├── execution/
│   ├── __init__.py
│   ├── executor.py          # Places/cancels orders
│
├── monitor/
│   ├── __init__.py
│   ├── logger.py            # Logging and dashboard
│
├── tests/
│   ├── test_reward_calc.py
│   ├── test_inventory.py
│   ├── test_quote_engine.py
│   ├── test_executor.py
│
├── requirements.in          # For uv to manage dependencies
├── README.md
```

---

## Development & Testing Standards

- Follow the coding standards in `docs/developer_guide/coding_standards.md`:
  - Use spaces only, 100 character line limit, American English.
  - Comments and docstrings: one blank line above, sentence case, concise, no emoji.
  - Python docstrings in imperative mood.
  - Use explicit `is None`/`is not None` checks for null, not truthiness.
  - Trailing commas for multiline parameter lists.
- Use `uv` for all Python dependency management.
- Run `make format` and `make pre-commit` before every commit.
- Use `pytest` for all Python tests, and aim for high coverage.
- Ensure the codebase remains portable across Linux, macOS, and Windows.
- Rust components should follow similar standards and be tested with `make cargo-test`.

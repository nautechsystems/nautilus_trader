# Trader control plane

The trader control plane is a read-only layer for dashboard, operations cockpit, source-health,
risk-cockpit, and future AI-assistant workflows. It is intentionally separate from the existing
trading engine, strategy APIs, execution semantics, risk decisions, and backtesting behavior.

## Repository discovery

Phase 1 found the current NautilusTrader layout already separates runtime concerns across these
modules:

- Runtime and kernel: `nautilus_trader/system/kernel.py`, `nautilus_trader/live/node.py`, and
  `nautilus_trader/trading/trader.py`.
- Message bus, actors, events, and components: `nautilus_trader/msgbus`,
  `nautilus_trader/common/actor.pyx`, `nautilus_trader/common/component.pyx`, and
  `nautilus_trader/common/events.py`.
- Data engines, clients, and adapters: `nautilus_trader/data`, `nautilus_trader/live/data_engine.py`,
  `nautilus_trader/live/data_client.py`, and `nautilus_trader/adapters`.
- Risk: `nautilus_trader/risk` and `nautilus_trader/live/risk_engine.py`.
- Portfolio: `nautilus_trader/portfolio`.
- Execution: `nautilus_trader/execution`, `nautilus_trader/live/execution_engine.py`, and
  `nautilus_trader/live/execution_client.py`.
- Cache and persistence: `nautilus_trader/cache` and `nautilus_trader/persistence`.
- Existing command surfaces: package-level `__main__.py` entry points exist for backtest and live
  nodes. Phase 1 therefore adds a minimal `python -m nautilus_trader.control_plane` command surface
  rather than introducing a web frontend or API server.

## Why this layer is read-only

The control plane exists to summarize observed state for traders. It must not place orders, mutate
strategy state, change risk decisions, or hide unavailable telemetry as healthy. The first phase uses
normalized snapshot inputs and immutable dashboard models so runtime integrations can feed already
observed state into the service without giving the dashboard authority over execution or risk.

## Relationship to engine domains

The service sits above runtime, data, portfolio, risk, execution, cache, and persistence domains. It
aggregates health and permission summaries from those domains when telemetry is available. When
telemetry is missing, it returns explicit `unknown` component statuses and degraded dashboard health
instead of failing or inventing a healthy state.

## System health states

- `normal`: Critical telemetry is present, dependencies report healthy states, and trading permission
  is allowed.
- `degraded`: One or more dependencies are unknown, stale, delayed, reconnecting, or only partially
  available. Traders should reduce attention to new signals and investigate the degraded component.
- `atRisk`: Active incidents or close-only risk posture indicate that the system can still be managed
  but should not be treated as fully healthy.
- `halted`: Execution, event-bus, data, persistence, or risk controls indicate that trading should be
  stopped or blocked.

## Trading permission states

- `allowed`: New signals and risk can continue under normal controls.
- `reducedSignals`: Dependencies are degraded or unknown, so traders should reduce signal intake and
  avoid adding unnecessary exposure.
- `closeOnly`: New risk should not be opened; operators should focus on reducing or closing existing
  risk until breaches or halted dependencies are resolved.
- `blocked`: Trading is blocked, typically because a kill switch, execution halt, or severe dependency
  failure is active.

## Future phases

- Web dashboard and read-only API endpoints.
- Dedicated operations cockpit views.
- Dedicated risk cockpit views with portfolio-aware exposure and drawdown summaries.
- Source-health dashboard backed by adapter- and data-client telemetry.
- AI assistant integration that consumes the deterministic context object without changing trading
  behavior.

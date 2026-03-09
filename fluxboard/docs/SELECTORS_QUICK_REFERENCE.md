<!-- DOCID: SELECTORS_QUICK_REFERENCE.md@v1 -->
# Zustand Selectors - Quick Reference

## Purpose

Provide a compact cheat sheet for using Fluxboard’s Zustand selectors without rereading the full guide.

## Scope

- Import patterns for selector-based subscriptions
- Store-by-store selector usage snippets
- Shallow comparison rules and common patterns

## References

- Full guide: `fluxboard/docs/SELECTORS_GUIDE.md`
- Migration summary: `fluxboard/docs/SELECTORS_MIGRATION_SUMMARY.md`
- Implementation: `fluxboard/stores.ts`

## Changelog

- 2025-11-20: Added standard doc sections; content otherwise unchanged.

## Import Pattern

```typescript
import { useStoreNameStore, selectStoreNameProperty, shallow } from './stores';
```text

## Usage Cheat Sheet

### ✅ DO - Use Selectors

```typescript
// Primitive values (no shallow needed)
const loading = useAlertsStore(selectAlertsLoading);
const count = useAlertsStore(selectAlertsCount);

// Arrays/Objects (always use shallow)
const rows = useTradesStore(selectTradesRows, shallow);
const prefs = useParamsStore(selectParamsColumnPrefs, shallow);

// Parameterized selectors (inline function)
const recent = useTradesStore(state => selectRecentTrades(state, 10), shallow);
const critical = useAlertsStore(state => selectAlertsBySeverity(state, 'CRITICAL'), shallow);
```text

### ❌ DON'T - Old Pattern

```typescript
// ❌ Re-renders on ANY store change
const { rows, loading } = useAlertsStore();
```text

## Store-by-Store Quick Reference

### Market Store

```typescript
const rows = useMarketStore(selectMarketRows, shallow);
const lastUpdate = useMarketStore(selectMarketLastUpdate);
const bybit = useMarketStore(state => selectMarketByExchange(state, 'bybit'), shallow);
```text

### Trades Store

```typescript
const rows = useTradesStore(selectTradesRows, shallow);
const top10 = useTradesStore(state => selectRecentTrades(state, 10), shallow);
const byStrat = useTradesStore(state => selectTradesByStrategy(state, 'my_strat'), shallow);
const lastUpdate = useTradesStore(selectTradesLastUpdate);
```text

### Alerts Store

```typescript
const rows = useAlertsStore(selectAlertsRows, shallow);
const undismissed = useAlertsStore(selectUndismissedAlerts, shallow);
const critical = useAlertsStore(state => selectAlertsBySeverity(state, 'CRITICAL'), shallow);
const loading = useAlertsStore(selectAlertsLoading);
const lastUpdate = useAlertsStore(selectAlertsLastUpdate);
```text

### Balances Store

```typescript
const rows = useBalancesStore(selectBalancesRows, shallow);
const bybit = useBalancesStore(state => selectBalancesByExchange(state, 'bybit'), shallow);
const ethTotal = useBalancesStore(state => selectTotalBalance(state, 'ETH'));
const loading = useBalancesStore(selectBalancesLoading);
```text

### Signal Store

```typescript
const rows = useSignalStore(selectSignalRows, shallow);
const active = useSignalStore(selectActiveStrategies, shallow);
const lastUpdate = useSignalStore(selectSignalLastUpdate);
```text

### FVs Store

```typescript
const rows = useFVsStore(selectFVsRows, shallow);
const stale = useFVsStore(selectStaleFVs, shallow);
```text

### FX Store

```typescript
const data = useFxStore(selectFxData);
const loading = useFxStore(selectFxLoading);
const ethPair = useFxStore(state => selectFxRate(state, 'ETH/USDT'));
```text

### Params Store

```typescript
const auto = useParamsStore(selectParamsAuto);
const viewMode = useParamsStore(selectParamsViewMode);
const columnPrefs = useParamsStore(selectParamsColumnPrefs, shallow);
const sortState = useParamsStore(selectParamsSortState, shallow);
```text

## Common Patterns

### Panel Components (Staleness Indicator)

```typescript
// Only subscribe to lastUpdate for staleness - ignore data
const lastUpdate = useTradesStore(selectTradesLastUpdate);
```text

### Table Components (Full Data)

```typescript
// Subscribe to full dataset with shallow comparison
const rows = useTradesStore(selectTradesRows, shallow);
```text

### Filtered Lists

```typescript
// Filter at selector level, not component level
const buys = useTradesStore(state => selectTradesBySide(state, 'buy'), shallow);
```text

### Derived Computations

```typescript
// Use useMemo for expensive calculations
const rows = useTradesStore(selectTradesRows, shallow);
const totalVolume = useMemo(() =>
  rows.reduce((sum, r) => sum + (r.mv || 0), 0),
  [rows]
);
```typescript

## When to Use `shallow`

| Data Type | Use `shallow`? | Example |
|-----------|----------------|---------|
| Primitive | ❌ No | `const count = useStore(selectCount);` |
| Array | ✅ Yes | `const rows = useStore(selectRows, shallow);` |
| Object | ✅ Yes | `const prefs = useStore(selectPrefs, shallow);` |
| Set | ✅ Yes | `const ids = useStore(selectIds, shallow);` |
| Map | ✅ Yes | `const map = useStore(selectMap, shallow);` |
| undefined/null | ❌ No | `const update = useStore(selectUpdate);` |

## All Selectors Reference

### Market (4)
- `selectMarketRows`
- `selectMarketLastUpdate`
- `selectMarketBySymbol(exchange, symbol)`
- `selectMarketByExchange(exchange)`

### Trades (9)
- `selectTradesRows`
- `selectTradesLastSeq`
- `selectTradesLastUpdate`
- `selectTradeById(rowId)`
- `selectRecentTrades(limit)`
- `selectTradesByStrategy(signalId)`
- `selectTradesByExchange(exchange)`
- `selectTradesByCoin(coin)`
- `selectTradesBySide(side)`

### FVs (3)
- `selectFVsRows`
- `selectFVBySymbol(symbol)`
- `selectStaleFVs(thresholdMs?)`

### FX (6)
- `selectFxData`
- `selectFxLoading`
- `selectFxError`
- `selectFxAuto`
- `selectFxLastFetch`
- `selectFxRate(pair)`

### Signal (5)
- `selectSignalRows`
- `selectSignalLastUpdate`
- `selectSignalById(id)`
- `selectActiveStrategies`
- `selectSignalByEdge(minEdge)`

### Balances (5)
- `selectBalancesRows`
- `selectBalancesLoading`
- `selectBalancesByExchange(exchange)`
- `selectBalancesByCoin(coin)`
- `selectTotalBalance(coin)`

### Params (7)
- `selectParamsAuto`
- `selectParamsViewMode`
- `selectParamsColumnPrefs`
- `selectParamsSortState`
- `selectParamsSelectedStrategies`
- `selectParamsLastFocusedCell`
- `selectParamsColumnVisibility(key)`

### Alerts (9)
- `selectAlertsRows`
- `selectAlertsLoading`
- `selectAlertsAuto`
- `selectAlertsLastUpdate`
- `selectAlertsDismissedIds`
- `selectAlertsBySeverity(severity)`
- `selectUndismissedAlerts`
- `selectAlertsCount`
- `selectAlertById(id)`

## Troubleshooting

**Component re-renders too often?**
→ Use React DevTools Profiler to identify which props are changing

**Forgot to use shallow for array?**
→ Array reference changes on every store update, causing re-render

**Creating new array in selector?**
→ Don't use spread/map in selector - return existing reference

**TypeScript error with shallow?**
→ Import from './stores', not directly from 'zustand/shallow'

## Full Documentation

- **Comprehensive Guide**: `docs/SELECTORS_GUIDE.md`
- **Migration Summary**: `docs/SELECTORS_MIGRATION_SUMMARY.md`
- **Implementation**: `stores.ts` (inline JSDoc)

import { afterEach, describe, expect, it } from 'vitest';
import { useMarketDataStore, MARKET_DATA_PAGE_SIZE } from './marketDataStore';

describe('marketDataStore', () => {
  afterEach(() => {
    useMarketDataStore.setState({ rows: [], loading: false, lastUpdate: null });
  });

  it('initializes with defaults and exposes page size constant', () => {
    const state = useMarketDataStore.getState();
    expect(state.rows).toEqual([]);
    expect(state.loading).toBe(false);
    expect(state.lastUpdate).toBeNull();
    expect(MARKET_DATA_PAGE_SIZE).toBe(50);
  });

  it('sets snapshot and updates lastUpdate', () => {
    const before = useMarketDataStore.getState().lastUpdate;
    useMarketDataStore.getState().setSnapshot([
      { coin: 'BTC/USDT', exchange: 'bybit', bid: '', bid_qty: '', mid_px: '101', ask: '', ask_qty: '', timestamp_ms: 123 },
    ]);
    const after = useMarketDataStore.getState().lastUpdate;
    expect(useMarketDataStore.getState().rows).toHaveLength(1);
    expect(after).not.toBe(before);
  });

  it('replaces from socket and accepts provided timestamp', () => {
    useMarketDataStore.getState().replaceFromSocket(
      [
        { coin: 'ETH/USDT', exchange: 'dex', bid: '', bid_qty: '', mid_px: '2000', ask: '', ask_qty: '', timestamp_ms: 456 },
      ],
      999,
    );
    const state = useMarketDataStore.getState();
    expect(state.rows[0].coin).toBe('ETH/USDT');
    expect(state.lastUpdate).toBe(999);
  });

  it('updates lastUpdate directly', () => {
    useMarketDataStore.getState().setLastUpdate(1234);
    expect(useMarketDataStore.getState().lastUpdate).toBe(1234);
  });

  it('toggles loading', () => {
    useMarketDataStore.getState().setLoading(true);
    expect(useMarketDataStore.getState().loading).toBe(true);
    useMarketDataStore.getState().setLoading(false);
    expect(useMarketDataStore.getState().loading).toBe(false);
  });
});

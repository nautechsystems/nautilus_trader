import { beforeEach, describe, expect, it } from 'vitest';
import { useFvStore } from '../stores';

describe('FvStore', () => {
  beforeEach(() => {
    useFvStore.setState({
      loading: false,
      error: undefined,
      profile: 'fv1',
      profiles: ['fv1'],
      symbols: [],
      symbol: undefined,
      latest: undefined,
      auto: true,
      intervalMs: 1000,
      backoffMs: 1000,
      lastFetchMs: undefined,
    });
  });

  it('stores latest snapshot and updates last fetch timestamp', () => {
    const startedAt = Date.now();
    useFvStore.getState().setLatest({
      symbol: 'BICO_USDT',
      ts_ms: 123,
      final: 1.2345,
      base: 1.2,
      signed_volume: 0.1,
      overlay_pct: 0.01,
      terms: [],
    });

    const state = useFvStore.getState();
    expect(state.latest?.symbol).toBe('BICO_USDT');
    expect(state.lastFetchMs).toBeGreaterThanOrEqual(startedAt);
  });

  it('sets symbol list and defaults selected symbol when empty', () => {
    useFvStore.getState().setSymbols(['BICO_USDT', 'ETH_USDT']);
    const state = useFvStore.getState();
    expect(state.symbols).toEqual(['BICO_USDT', 'ETH_USDT']);
    expect(state.symbol).toBe('BICO_USDT');
  });

  it('defaults to fv1 profile and supports explicit profile selection', () => {
    const state = useFvStore.getState() as unknown as {
      profile?: string;
      setProfile?: (profile: string) => void;
    };
    expect(state.profile).toBe('fv1');
    expect(typeof state.setProfile).toBe('function');
    state.setProfile?.('fv2');
    const nextState = useFvStore.getState() as unknown as { profile?: string };
    expect(nextState.profile).toBe('fv2');
  });
});

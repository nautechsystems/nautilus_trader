/**
 * Unit tests for SignalTable store merge logic
 *
 * Tests the deep merge behavior for signal strategy updates,
 * ensuring legs are properly merged without data loss.
 */

import { describe, it, expect, beforeEach } from 'vitest';

// Import store module
let storeModule: typeof import('../../../stores');

describe('SignalTable Store Merge Logic', () => {
  beforeEach(async () => {
    // Use real store for these tests
    vi.restoreAllMocks();
    vi.doUnmock('../../../stores');
    vi.resetModules();
    storeModule = await import('../../../stores');
  });

  describe('Deep merge for legs', () => {
    it('deep merges leg properties without losing existing data', () => {
      const { useSignalStore } = storeModule;

      const initial: any = {
        id: 'deep_merge_test',
        params: { bot_on: '1' },
        legs: {
          A: {
            coin: 'BTC',
            exchange: 'bybit',
            decision_bid: 50000,
            decision_ask: 50100,
            raw_bid: 49950,
            raw_ask: 50150,
            fee_bps: 10,
            update_time: '2025-01-15 12:00:00'
          },
          B: {
            coin: 'BTC',
            exchange: 'dex',
            decision_bid: 50050,
            decision_ask: 50150,
            update_time: '2025-01-15 12:00:01'
          }
        },
        balances_ok: true
      };

      const store = useSignalStore.getState();
      store.setRows([initial]);

      // Partial update: only updates decision_bid for leg A
      const delta: any = {
        id: 'deep_merge_test',
        legs: {
          A: {
            decision_bid: 50010 // Only this field changes
            // Other fields should be preserved
          }
        }
      };

      store.mergeStrategy(delta);

      const state = useSignalStore.getState();
      const merged = state.rows.find(r => r.id === 'deep_merge_test')!;

      // Verify deep merge: new value applied
      expect(merged.legs.A.decision_bid).toBe(50010);

      // Verify deep merge: existing properties preserved
      expect(merged.legs.A.decision_ask).toBe(50100);
      expect(merged.legs.A.raw_bid).toBe(49950);
      expect(merged.legs.A.fee_bps).toBe(10);
      expect(merged.legs.A.update_time).toBe('2025-01-15 12:00:00');

      // Verify leg B unchanged
      expect(merged.legs.B.decision_bid).toBe(50050);
    });

    it('only patches legs that exist in delta', () => {
      const { useSignalStore } = storeModule;

      const initial: any = {
        id: 'partial_patch_test',
        params: { bot_on: '1' },
        legs: {
          A: {
            coin: 'BTC',
            exchange: 'bybit',
            decision_bid: 50000,
            decision_ask: 50100
          },
          B: {
            coin: 'BTC',
            exchange: 'dex',
            decision_bid: 50050,
            decision_ask: 50150
          }
        },
        balances_ok: true
      };

      const store = useSignalStore.getState();
      store.setRows([initial]);

      // Delta only includes leg A
      const delta: any = {
        id: 'partial_patch_test',
        legs: {
          A: {
            decision_bid: 50010
          }
          // B is not included - should remain unchanged
        }
      };

      store.mergeStrategy(delta);

      const state = useSignalStore.getState();
      const merged = state.rows.find(r => r.id === 'partial_patch_test')!;

      // Leg A updated
      expect(merged.legs.A.decision_bid).toBe(50010);

      // Leg B unchanged (not in delta)
      expect(merged.legs.B.decision_bid).toBe(50050);
      expect(merged.legs.B.decision_ask).toBe(50150);
    });

    it('handles null leg deletion by removing the key', () => {
      const { useSignalStore } = storeModule;

      const initial: any = {
        id: 'delete_leg_test',
        params: { bot_on: '1' },
        legs: {
          A: {
            coin: 'BTC',
            exchange: 'bybit',
            decision_bid: 50000,
            decision_ask: 50100
          },
          B: {
            coin: 'BTC',
            exchange: 'dex',
            decision_bid: 50050,
            decision_ask: 50150
          }
        },
        balances_ok: true
      };

      const store = useSignalStore.getState();
      store.setRows([initial]);

      // Explicitly delete leg A
      const delta: any = {
        id: 'delete_leg_test',
        legs: {
          A: null // Explicit deletion
        }
      };

      store.mergeStrategy(delta);

      const state = useSignalStore.getState();
      const merged = state.rows.find(r => r.id === 'delete_leg_test')!;

      // Leg A key should be removed (not retained as a null tombstone)
      expect(merged.legs.A).toBeUndefined();
      expect('A' in merged.legs).toBe(false);

      // Leg B unchanged
      expect(merged.legs.B.decision_bid).toBe(50050);
    });

    it('clears legs_order when update sets legs_order to null', () => {
      const { useSignalStore } = storeModule;

      const initial: any = {
        id: 'clear_legs_order_test',
        params: { bot_on: '1' },
        legs_order: ['contract_z', 'contract_a'],
        legs: {
          contract_z: {
            coin: 'BTC',
            exchange: 'bybit',
            decision_bid: 50000,
            decision_ask: 50100,
          },
          contract_a: {
            coin: 'BTC',
            exchange: 'dex',
            decision_bid: 50050,
            decision_ask: 50150,
          },
        },
        balances_ok: true,
      };

      useSignalStore.getState().setRows([initial]);

      useSignalStore.getState().mergeStrategy({
        id: 'clear_legs_order_test',
        legs_order: null,
      } as any);

      const merged = useSignalStore.getState().rows.find((r) => r.id === 'clear_legs_order_test')!;
      expect(merged.legs_order).toBeNull();
    });

    it('preserves legs when delta has no legs property', () => {
      const { useSignalStore } = storeModule;

      const initial: any = {
        id: 'no_legs_delta_test',
        params: { bot_on: '1' },
        legs: {
          A: {
            coin: 'BTC',
            exchange: 'bybit',
            decision_bid: 50000,
            decision_ask: 50100
          },
          B: {
            coin: 'BTC',
            exchange: 'dex',
            decision_bid: 50050,
            decision_ask: 50150
          }
        },
        balances_ok: true
      };

      const store = useSignalStore.getState();
      store.setRows([initial]);

      // Delta without legs property
      const delta: any = {
        id: 'no_legs_delta_test',
        decision_edge_bps: 10.5
        // No legs property
      };

      store.mergeStrategy(delta);

      const state = useSignalStore.getState();
      const merged = state.rows.find(r => r.id === 'no_legs_delta_test')!;

      // Legs should be preserved
      expect(merged.legs.A.decision_bid).toBe(50000);
      expect(merged.legs.B.decision_bid).toBe(50050);

      // Other fields updated
      expect(merged.decision_edge_bps).toBe(10.5);
    });

    it('deep merges contract_id keyed legs without clobbering same-exchange siblings', () => {
      const { useSignalStore } = storeModule;

      const initial: any = {
        id: 'contract_id_merge_test',
        params: { bot_on: '1' },
        legs_order: ['BTCUSDT-PERP', 'BTCUSDT-SPOT'],
        legs: {
          'BTCUSDT-PERP': {
            coin: 'BTC/USDT',
            exchange: 'bybit',
            decision_bid: 50000,
            decision_ask: 50100,
            raw_bid: 49990,
          },
          'BTCUSDT-SPOT': {
            coin: 'BTC/USDT',
            exchange: 'bybit',
            decision_bid: 50010,
            decision_ask: 50110,
          },
        },
        balances_ok: true,
      };

      useSignalStore.getState().setRows([initial]);

      const delta: any = {
        id: 'contract_id_merge_test',
        legs: {
          'BTCUSDT-PERP': {
            decision_bid: 50025,
          },
        },
      };

      useSignalStore.getState().mergeStrategy(delta);

      const merged = useSignalStore.getState().rows.find((r) => r.id === 'contract_id_merge_test')!;
      expect(merged.legs['BTCUSDT-PERP']?.decision_bid).toBe(50025);
      expect(merged.legs['BTCUSDT-PERP']?.decision_ask).toBe(50100);
      expect(merged.legs['BTCUSDT-SPOT']?.decision_bid).toBe(50010);
      expect(merged.legs['BTCUSDT-SPOT']?.exchange).toBe('bybit');
    });

    it('deep merges shared equities_arb payloads without dropping operator or quote snapshot fields', () => {
      const { useSignalStore } = storeModule;

      useSignalStore.getState().setRows([
        {
          id: 'aapl_tradexyz_maker',
          strategy_family: 'equities_maker',
          params: { bot_on: '1', qty: '1' },
          legs: {},
          balances_ok: true,
          equities_arb: {
            operator: {
              execution_mode: 'maker_hedge',
              behavior: 'maker',
              hedge_policy: {
                route: 'SMART',
                time_in_force: 'DAY',
              },
            },
            quote_snapshot: {
              ts_ms: 1_700_000_000_500,
              effective_spread_bps: 6.5,
              maker_leg: {
                instrument_id: 'xyz:AAPL-USD-PERP.HYPERLIQUID',
                quote_state: 'fresh',
              },
            },
          },
        } as any,
      ]);

      useSignalStore.getState().mergeStrategy({
        id: 'aapl_tradexyz_maker',
        equities_arb: {
          quote_snapshot: {
            hedge_latency_ms: 45,
            maker_leg: {
              feed_state: 'ok',
            },
          },
        },
      } as any);

      const merged = useSignalStore.getState().rows.find((row) => row.id === 'aapl_tradexyz_maker') as any;
      expect(merged.equities_arb.operator).toMatchObject({
        execution_mode: 'maker_hedge',
        behavior: 'maker',
        hedge_policy: {
          route: 'SMART',
          time_in_force: 'DAY',
        },
      });
      expect(merged.equities_arb.quote_snapshot).toMatchObject({
        effective_spread_bps: 6.5,
        hedge_latency_ms: 45,
      });
      expect(merged.equities_arb.quote_snapshot.maker_leg).toMatchObject({
        instrument_id: 'xyz:AAPL-USD-PERP.HYPERLIQUID',
        quote_state: 'fresh',
        feed_state: 'ok',
      });
    });
  });

  describe('Batched merge behavior', () => {
    it('matches sequential mergeStrategy results', () => {
      const { useSignalStore } = storeModule;

      const initialRows: any[] = [
        {
          id: 'batch_a',
          params: { bot_on: '1', qty: '100' },
          legs: {
            A: { coin: 'BTC', exchange: 'bybit', decision_bid: 50000, decision_ask: 50100 },
            B: { coin: 'BTC', exchange: 'dex', decision_bid: 50050, decision_ask: 50150 },
          },
          balances_ok: true,
          decision_edge_bps: 12.0,
          required_edge_bps: 8.0,
          edge2_bps: 4.0,
        },
        {
          id: 'batch_b',
          params: { bot_on: '0', qty: '50' },
          legs: {
            A: { coin: 'ETH', exchange: 'bybit', decision_bid: 2500, decision_ask: 2501 },
            B: { coin: 'ETH', exchange: 'dex', decision_bid: 2502, decision_ask: 2503 },
          },
          balances_ok: false,
        },
      ];

      const updates: any[] = [
        {
          id: 'batch_a',
          decision_edge_bps: 13.5,
          legs: {
            A: { decision_bid: 50010 },
          },
          maker_quote_status: { bid_open: 1 },
        },
        {
          id: 'batch_b',
          required_edge_bps: 5,
          decision_edge_bps: 8,
          params: { bot_on: '1' },
          pricing_adjustments: [{ type: 'inventory_skew', inv_ratio: 1.2 }],
        },
        {
          id: 'batch_c',
          params: { bot_on: '1', qty: '10' },
          legs: {
            A: { coin: 'SEI', exchange: 'bybit', decision_bid: 1.01, decision_ask: 1.02 },
            B: null,
          },
          balances_ok: true,
        },
      ];

      useSignalStore.setState({ rows: [], lastUpdate: undefined });
      useSignalStore.getState().setRows(initialRows as any);
      updates.forEach((update) => useSignalStore.getState().mergeStrategy(update as any));
      const sequentialRows = JSON.parse(JSON.stringify(useSignalStore.getState().rows));

      useSignalStore.setState({ rows: [], lastUpdate: undefined });
      useSignalStore.getState().setRows(initialRows as any);
      useSignalStore.getState().mergeStrategies(updates as any);
      const batchedRows = useSignalStore.getState().rows;

      expect(batchedRows).toEqual(sequentialRows);
    });

    it('notifies subscribers once for mergeStrategies vs once per mergeStrategy call', () => {
      const { useSignalStore } = storeModule;

      const initial: any = {
        id: 'batch_notify',
        params: { bot_on: '1', qty: '100' },
        legs: {
          A: { coin: 'BTC', exchange: 'bybit', decision_bid: 50000, decision_ask: 50100 },
          B: { coin: 'BTC', exchange: 'dex', decision_bid: 50050, decision_ask: 50150 },
        },
        balances_ok: true,
      };

      const updates: any[] = [
        { id: 'batch_notify', decision_edge_bps: 10 },
        { id: 'batch_notify', required_edge_bps: 6 },
        { id: 'batch_notify', maker_quote_status: { bid_open: 2 } },
      ];

      useSignalStore.setState({ rows: [], lastUpdate: undefined });
      useSignalStore.getState().setRows([initial]);
      let sequentialNotifyCount = 0;
      const unsubscribeSequential = useSignalStore.subscribe(() => {
        sequentialNotifyCount += 1;
      });
      updates.forEach((update) => useSignalStore.getState().mergeStrategy(update as any));
      unsubscribeSequential();

      useSignalStore.setState({ rows: [], lastUpdate: undefined });
      useSignalStore.getState().setRows([initial]);
      let batchedNotifyCount = 0;
      const unsubscribeBatched = useSignalStore.subscribe(() => {
        batchedNotifyCount += 1;
      });
      useSignalStore.getState().mergeStrategies(updates as any);
      unsubscribeBatched();

      expect(sequentialNotifyCount).toBe(updates.length);
      expect(batchedNotifyCount).toBe(1);
    });
  });

  describe('Params store migration', () => {
    it('rehydrates an existing version 4 params store payload instead of discarding it', async () => {
      localStorage.setItem(
        'fluxboard:params:ui:v1',
        JSON.stringify({
          state: {
            activeProfile: 'equities_taker',
            columnPrefsByProfile: {
              equities_taker: {
                order: ['qty', 'bid_edge_take_bps'],
                visibility: { bid_edge_take_bps: true },
              },
            },
            sortState: { key: 'qty', direction: 'desc' },
          },
          version: 4,
        }),
      );

      vi.resetModules();
      const { useParamsStore } = await import('../../../stores');
      const state = useParamsStore.getState();

      expect(state.activeProfile).toBe('equities_taker');
      expect(state.columnPrefs.order).toEqual(['qty', 'bid_edge_take_bps']);
      expect(state.columnPrefs.visibility).toMatchObject({ bid_edge_take_bps: true });
      expect(state.sortState).toEqual({ key: 'qty', direction: 'desc' });
    });

    it('migrates a legacy maker_v4 active profile onto equities_maker while copying its params preferences onto the split equities profiles', async () => {
      localStorage.setItem(
        'fluxboard:params:ui:v1',
        JSON.stringify({
          state: {
            activeProfile: 'maker_v4',
            columnPrefsByProfile: {
              maker_v4: {
                order: ['hedge_style', 'assumed_hedge_fee_bps'],
                visibility: { assumed_hedge_fee_bps: true },
              },
            },
            sortState: { key: null, direction: null },
          },
          version: 3,
        }),
      );

      vi.resetModules();
      const { useParamsStore } = await import('../../../stores');
      const state = useParamsStore.getState();

      expect(state.activeProfile).toBe('equities_maker');
      expect(state.columnPrefsByProfile.equities_maker.order).toEqual([
        'hedge_style',
        'assumed_hedge_fee_bps',
      ]);
      expect(state.columnPrefsByProfile.equities_taker.order).toEqual([
        'hedge_style',
        'assumed_hedge_fee_bps',
      ]);
      expect(state.columnPrefs.order).toEqual([
        'hedge_style',
        'assumed_hedge_fee_bps',
      ]);
      expect(state.columnPrefsByProfile.equities_maker.visibility).toMatchObject({
        assumed_hedge_fee_bps: true,
      });
    });
  });
});

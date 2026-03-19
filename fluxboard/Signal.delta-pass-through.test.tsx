import { act, render, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import SignalTable from '@/components/domain/signal/SignalTable';
import * as apiModule from '@/api';
import * as socketsModule from '@/sockets';
import { useSignalStore } from '@/stores';
import type { SignalStrategy } from '@/types';

vi.mock('@/api', () => ({
  api: {
    getSignalStrategies: vi.fn(),
  },
}));

vi.mock('@/sockets', () => ({
  socket: {
    on: vi.fn(),
    off: vi.fn(),
    connected: false,
  },
}));

vi.mock('@/stores', async () => {
  const actual = await vi.importActual<any>('@/stores');
  return { ...actual, useSignalStore: vi.fn() };
});

let currentSignalState: any;

function initSignalState(state: any) {
  currentSignalState = {
    rows: [],
    setRows: vi.fn(),
    mergeStrategy: vi.fn(),
    mergeStrategies: vi.fn(),
    ...state,
  };

  (useSignalStore as any).mockImplementation((selector?: any) =>
    selector ? selector(currentSignalState) : currentSignalState,
  );
  (useSignalStore as any).getState = () => currentSignalState;
}

describe('signal_delta field pass-through wiring', () => {
  const mockMergeStrategy = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();

    (apiModule.api.getSignalStrategies as any).mockResolvedValue({
      strategies: [],
      server_time: '2026-03-01 12:00:00',
      server_ts_ms: Date.now(),
    });

    const seeded: SignalStrategy = {
      id: 'pass_through_strategy',
      params: { bot_on: '0', qty: '10' } as any,
      legs: {
        A: {
          coin: 'PLUME/USDT',
          exchange: 'bybit_linear',
          update_time: '2026-03-01 12:00:00',
        } as any,
        B: null,
      },
      balances_ok: false,
      meta: {
        class: 'equities_maker',
        param_set: 'equities_maker',
        strategy_family: 'equities_maker',
      },
    } as any;

    initSignalState({
      rows: [seeded],
      mergeStrategy: mockMergeStrategy,
    });
  });

  it('passes through shared equities-arb payload, params, balance_readiness, balances_ok, and last_trade on signal_delta', async () => {
    render(
      <MemoryRouter initialEntries={['/equities/signal']}>
        <SignalTable />
      </MemoryRouter>,
    );

    await waitFor(() => {
      const hasDeltaHandler = (socketsModule.socket.on as any).mock.calls.some(
        (call: any[]) => call[0] === 'signal_delta',
      );
      expect(hasDeltaHandler).toBe(true);
    });

    const deltaHandler = (socketsModule.socket.on as any).mock.calls.find(
      (call: any[]) => call[0] === 'signal_delta',
    )?.[1];
    expect(typeof deltaHandler).toBe('function');

    act(() => {
      deltaHandler({
        id: 'pass_through_strategy',
        legs: {
          'bybit_linear:PLUMEUSDT': {
            symbol: 'PLUMEUSDT',
            decision_bid: 0.0095,
            decision_ask: 0.0096,
            update_ts_ms: 1736942400000,
          },
        },
        params: { bot_on: '1', qty: '25' },
        balance_readiness: {
          status: 'WARN',
          summary: 'Low inventory on one venue',
          requirements: [
            {
              location: 'bybit',
              token: 'PLUME',
              required: 10,
              available: 7,
              coverage: 0.7,
            },
          ],
        },
        balances_ok: true,
        strategy_family: 'equities_maker',
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
            ts_ms: 1736942400000,
            effective_spread_bps: 6.5,
            hedge_latency_ms: 45,
          },
        },
        meta: {
          class: 'equities_maker',
          external_strategy_id: 'equities_maker_external',
        },
        last_trade: {
          side: 'buy',
          price: 0.0095,
          qty: 250,
          ts_ms: 1736942400000,
        },
      });
    });

    expect(mockMergeStrategy).toHaveBeenCalled();
    const merged = mockMergeStrategy.mock.calls.at(-1)?.[0];
    expect(merged?.id).toBe('pass_through_strategy');
    expect(merged?.params).toEqual({ bot_on: '1', qty: '25' });
    expect(merged?.balance_readiness).toMatchObject({
      status: 'WARN',
      summary: 'Low inventory on one venue',
    });
    expect(merged?.balances_ok).toBe(true);
    expect(merged?.strategy_family).toBe('equities_maker');
    expect(merged?.equities_arb).toMatchObject({
      operator: {
        execution_mode: 'maker_hedge',
        behavior: 'maker',
      },
      quote_snapshot: {
        effective_spread_bps: 6.5,
        hedge_latency_ms: 45,
      },
    });
    expect(merged?.meta).toMatchObject({
      class: 'equities_maker',
      external_strategy_id: 'equities_maker_external',
    });
    expect(merged?.last_trade).toMatchObject({
      side: 'buy',
      price: 0.0095,
      qty: 250,
      ts_ms: 1736942400000,
    });
    expect(merged?.legs).toBeTruthy();
    expect((merged as any).legs['bybit_linear:PLUMEUSDT']).toMatchObject({
      contract_id: 'bybit_linear:PLUMEUSDT',
      exchange: 'bybit_linear',
      coin: 'PLUME',
    });
  });
});

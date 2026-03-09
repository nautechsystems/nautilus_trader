import { beforeEach, describe, expect, it, vi } from 'vitest';

const fetchJSONMock = vi.hoisted(() => vi.fn());

vi.mock('../apiClient', () => {
  class MockAPIClient {
    fetchJSON(path: string, init?: RequestInit) {
      return fetchJSONMock(path, init);
    }
  }
  return { APIClient: MockAPIClient };
});

import { api } from '../api';

describe('api.getOrderViewSnapshot', () => {
  beforeEach(() => {
    fetchJSONMock.mockReset();
    fetchJSONMock.mockResolvedValue({
      ok: true,
      data: {
        room_id: 'order_view:strat-1:maker:book:0:depth:20',
        server_ts_ms: 1700000000000,
        selection: { strategy_id: 'strat-1', leg: 'maker' },
        context: {
          maker: { exchange: 'bybit_linear', symbol: 'BTC_USDT' },
          hedge: { exchange: 'binance_spot', symbol: 'BTC_USDT' },
        },
        state_rev: 'abc123',
        maker_state_ts_ms: 1700000000000,
        bbo: {
          maker: { bid: 30000, ask: 30010, mid: 30005, ts_ms: 1700000000000 },
        },
        open_orders: { rows: [] },
        events: { rows: [] },
        status: {
          md_ok: true,
          maker_state_ok: true,
          events_ok: true,
          last_md_ts_ms: 1700000000000,
          last_state_ts_ms: 1700000000000,
          notes: [],
        },
      },
      error: null,
    });
  });

  it('builds snapshot query string with caps-friendly params', async () => {
    await (api as any).getOrderViewSnapshot({
      strategyId: 'strat-1',
      leg: 'both',
      includeEvents: false,
      eventsLimit: 250,
      includeBook: true,
      bookDepth: 40,
      orderViewV02: true,
    });

    expect(fetchJSONMock).toHaveBeenCalledTimes(1);
    const [path] = fetchJSONMock.mock.calls[0];
    const search = String(path).split('?')[1] ?? '';
    const params = new URLSearchParams(search);

    expect(params.get('strategy_id')).toBe('strat-1');
    expect(params.get('leg')).toBe('both');
    expect(params.get('include_events')).toBe('0');
    expect(params.get('events_limit')).toBe('250');
    expect(params.get('include_book')).toBe('1');
    expect(params.get('book_depth')).toBe('40');
    expect(params.get('order_view_v02')).toBe('1');
  });

  it('defaults order_view_v02 to 0 when not provided', async () => {
    await (api as any).getOrderViewSnapshot({ strategyId: 'strat-1' });
    const [path] = fetchJSONMock.mock.calls[0];
    const search = String(path).split('?')[1] ?? '';
    const params = new URLSearchParams(search);
    expect(params.get('order_view_v02')).toBe('0');
  });

  it('unwraps Flux envelope payload', async () => {
    const result = await (api as any).getOrderViewSnapshot({ strategyId: 'strat-1' });

    expect(result.room_id).toBe('order_view:strat-1:maker:book:0:depth:20');
    expect(result.selection.strategy_id).toBe('strat-1');
  });

  it('throws backend error code when Flux envelope is not ok', async () => {
    fetchJSONMock.mockResolvedValue({ ok: false, data: null, error: 'not_found' });

    await expect((api as any).getOrderViewSnapshot({ strategyId: 'missing' })).rejects.toThrow(
      'not_found'
    );
  });
});

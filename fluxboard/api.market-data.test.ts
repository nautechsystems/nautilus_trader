import { beforeEach, describe, expect, it, vi } from 'vitest';

const fetchJSONMock = vi.hoisted(() => vi.fn());

vi.mock('./apiClient', () => {
  class MockAPIClient {
    fetchJSON(path: string, init?: RequestInit) {
      return fetchJSONMock(path, init);
    }
  }
  return { APIClient: MockAPIClient };
});

import { api } from './api';

describe('api.getMarketDataSnapshot', () => {
  beforeEach(() => {
    fetchJSONMock.mockReset();
    fetchJSONMock.mockResolvedValue({
      ok: true,
      data: {
        rows: [
          {
            coin: 'BTC/USDT',
            exchange: 'bybit',
            bid_px: '100',
            mid_px: '101',
            ask_px: '102',
            bid_qty: '1',
            ask_qty: '2',
            timestamp: 1700000000000,
          },
        ],
        count: 1,
      },
    });
  });

  it('requests snapshot endpoint and normalizes timestamp', async () => {
    const result = await api.getMarketDataSnapshot();

    expect(fetchJSONMock).toHaveBeenCalledWith('/api/v1/market-data/snapshot', undefined);
    expect(result.count).toBe(1);
    expect(result.rows[0].timestamp_ms).toBe(1700000000000);
  });

  it('preserves provided timestamp_ms', async () => {
    fetchJSONMock.mockResolvedValue({
      ok: true,
      data: {
        rows: [
          {
            coin: 'ETH/USDT',
            exchange: 'dex',
            bid_px: '',
            mid_px: '2000',
            ask_px: '',
            bid_qty: '',
            ask_qty: '',
            timestamp_ms: 1700000001000,
          },
        ],
        count: 1,
      },
    });

    const result = await api.getMarketDataSnapshot();
    expect(result.rows[0].timestamp_ms).toBe(1700000001000);
  });

  it('maps bid/ask fields from bid_px/ask_px', async () => {
    fetchJSONMock.mockResolvedValue({
      ok: true,
      data: {
        rows: [
          {
            coin: 'SOL/USDT',
            exchange: 'bybit',
            bid_px: '10',
            ask_px: '11',
            mid: '10.5',
            bid_size: '5',
            ask_size: '6',
            timestamp_ms: 1700000002000,
          },
        ],
        count: 1,
      },
    });

    const result = await api.getMarketDataSnapshot();
    expect(result.rows[0].bid).toBe('10');
    expect(result.rows[0].ask).toBe('11');
    expect(result.rows[0].mid_px).toBe('10.5');
    expect(result.rows[0].bid_qty).toBe('5');
    expect(result.rows[0].ask_qty).toBe('6');
  });

  it('passes through optional freshness metadata', async () => {
    fetchJSONMock.mockResolvedValue({
      ok: true,
      data: {
        rows: [
          {
            coin: 'PLUME/USDT',
            exchange: 'bybit',
            bid_px: '1.0',
            ask_px: '1.1',
            mid_px: '1.05',
            bid_qty: '10',
            ask_qty: '9',
            timestamp_ms: 1700000003000,
          },
        ],
        count: 1,
        freshness_key: 'md-key-abc',
        etag: '\"etag-md-abc\"',
        last_update_ms: 1700000003000,
      },
    });

    const result = await api.getMarketDataSnapshot();
    expect(result.freshnessKey).toBe('md-key-abc');
    expect(result.etag).toBe('\"etag-md-abc\"');
    expect(result.lastUpdateMs).toBe(1700000003000);
  });
});

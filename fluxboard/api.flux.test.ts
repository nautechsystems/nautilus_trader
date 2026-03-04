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

function setPathname(pathname: string) {
  (window.location as unknown as { pathname?: string }).pathname = pathname;
}

describe('api.getTrades', () => {
  beforeEach(() => {
    fetchJSONMock.mockReset();
    setPathname('/');
    fetchJSONMock.mockResolvedValue({
      ok: true,
      data: {
        rows: [],
        total: 0,
        limit: 50,
        offset: 0,
        has_more: true,
        next_offset: 50,
        next_cursor: 'cursor-token',
      },
    });
  });

  it('sends FluxAPI pagination params (limit/offset) instead of legacy page fields', async () => {
    await api.getTrades(3, 25, { sort: 'ts_desc' });

    expect(fetchJSONMock).toHaveBeenCalledTimes(1);
    const [path] = fetchJSONMock.mock.calls[0];
    const search = (path as string).split('?')[1] ?? '';
    const params = new URLSearchParams(search);

    expect(params.get('limit')).toBe('25');
    expect(params.get('offset')).toBe('50');
    expect(params.has('page')).toBe(false);
    expect(params.has('page_size')).toBe(false);
  });

  it('returns pagination metadata when provided by FluxAPI', async () => {
    const result = await api.getTrades(1, 50, { sort: 'ts_desc' });

    expect(result.has_more).toBe(true);
    expect(result.next_offset).toBe(50);
    expect(result.next_cursor).toBe('cursor-token');
  });

  it('passes cursor param when present', async () => {
    await api.getTrades(1, 50, { cursor: 'abc', sort: 'ts_desc' });

    const [path] = fetchJSONMock.mock.calls[0];
    expect(path).toContain('cursor=abc');
    expect(path).toContain('offset=0');
  });

  it('appends tokenmm profile for tokenmm routes', async () => {
    setPathname('/tokenmm/trades');

    await api.getTrades(1, 25, { sort: 'ts_desc' });

    const [path] = fetchJSONMock.mock.calls[0];
    const search = (path as string).split('?')[1] ?? '';
    const params = new URLSearchParams(search);
    expect(params.get('profile')).toBe('tokenmm');
  });

  it('normalizes flat trade rows without op/seq into TradeEvent rows', async () => {
    fetchJSONMock.mockResolvedValueOnce({
      ok: true,
      data: {
        rows: [
          {
            entry_id: '1772623943817-0',
            ts_ms: 1772623943812,
            version: 1,
            strategy_id: 'makerv3',
            instrument_id: 'PLUMEUSDT-LINEAR.BYBIT',
            side: '1',
            price: '0.009685',
            qty: '1000',
            trade_id: 'abc-trade',
          },
        ],
        total: 1,
        limit: 50,
        offset: 0,
      },
    });

    const result = await api.getTrades(1, 50, { sort: 'ts_desc' });
    expect(result.rows).toHaveLength(1);
    expect(result.rows[0]).toMatchObject({
      op: 'upsert',
      row_id: 'abc-trade',
      seq: 1772623943817,
      signal_id: 'makerv3',
      coin: 'PLUME',
      exchange: 'bybit',
      side: 'buy',
    });
    expect(result.rows[0]?.mv).toBeCloseTo(9.685, 10);
  });
});

describe('profile-scoped read APIs', () => {
  beforeEach(() => {
    fetchJSONMock.mockReset();
    setPathname('/');
  });

  it('appends profile to signals request on equities routes', async () => {
    setPathname('/equities/signal');
    fetchJSONMock.mockResolvedValue({ ok: true, data: { strategies: [] } });

    await api.getSignals();

    const [path] = fetchJSONMock.mock.calls[0];
    expect(path).toContain('/api/v1/signals?');
    expect(path).toContain('profile=equities');
  });

  it('appends profile to params request on tokenmm routes', async () => {
    setPathname('/tokenmm/params');
    fetchJSONMock.mockResolvedValue({ ok: true, data: [] });

    await api.getParams();

    const [path] = fetchJSONMock.mock.calls[0];
    expect(path).toContain('/api/v1/params?');
    expect(path).toContain('profile=tokenmm');
  });

  it('normalizes tokenmm signal payloads with state bot_on and bid/ask legs', async () => {
    setPathname('/tokenmm/signal');
    fetchJSONMock.mockResolvedValue({
      ok: true,
      data: {
        server_ts_ms: 1772623963299,
        strategies: [
          {
            id: 'makerv3',
            meta: { class: 'maker_v3' },
            state: { bot_on: true },
            legs: {
              'bybit:PLUMEUSDT': {
                exchange: 'bybit',
                symbol: 'PLUMEUSDT',
                bid: 0.009701,
                ask: 0.009702,
                ts_ms: 1772623962721,
              },
            },
          },
        ],
      },
    });

    const result = await api.getSignals();
    expect(result.strategies).toHaveLength(1);
    expect(result.strategies[0].params.bot_on).toBe('1');
    expect(result.strategies[0].legs['bybit:PLUMEUSDT']).toMatchObject({
      decision_bid: 0.009701,
      decision_ask: 0.009702,
      fv_bid: 0.009701,
      fv_ask: 0.009702,
      coin: 'PLUME',
      update_ts_ms: 1772623962721,
    });
  });

  it('prefers canonical strategy_id over stale id field in signal payloads', async () => {
    setPathname('/tokenmm/signal');
    fetchJSONMock.mockResolvedValueOnce({
      ok: true,
      data: {
        strategies: [
          {
            id: 'legacy-name',
            strategy_id: 'makerv3',
            meta: { strategy_id: 'makerv3', class: 'maker_v3' },
            state: { bot_on: false },
          },
        ],
      },
    });

    const result = await api.getSignals();
    expect(result.strategies).toHaveLength(1);
    expect(result.strategies[0].id).toBe('makerv3');
  });

  it('normalizes minimal Flux params schema into ParamDef fields used by Params UI', async () => {
    setPathname('/tokenmm/params');
    fetchJSONMock.mockResolvedValueOnce({
      ok: true,
      data: {
        params: {
          bot_on: {
            type: 'boolean',
            description: 'Enable quote publishing and management.',
          },
          qty: {
            type: 'number',
            description: 'Target base quantity per quote/hedge cycle.',
            minimum: 0,
            maximum: 1000,
          },
        },
        deprecated: {},
      },
    });

    const schema = await api.getParamSchema();
    expect(schema.params.bot_on).toMatchObject({
      key: 'bot_on',
      label: 'Enable quote publishing and management.',
      type: 'select',
      options: [['0', 'Off (0)'], ['1', 'On (1)']],
    });
    expect(schema.params.qty).toMatchObject({
      key: 'qty',
      label: 'Target base quantity per quote/hedge cycle.',
      type: 'float',
      min_value: 0,
      max_value: 1000,
    });
  });

  it('normalizes typed params payload values to string map used by Params editor', async () => {
    setPathname('/tokenmm/params');
    fetchJSONMock.mockResolvedValueOnce({
      ok: true,
      data: [
        {
          strategy_id: 'makerv3',
          params: {
            bot_on: false,
            qty: 1000,
            max_age_ms: 10000,
          },
        },
      ],
    });

    const rows = await api.getParams();
    expect(rows).toHaveLength(1);
    expect(rows[0].params).toMatchObject({
      bot_on: '0',
      qty: '1000',
      max_age_ms: '10000',
    });
  });

  it('derives trade mv from price*qty when incoming notional/mv is zero', async () => {
    fetchJSONMock.mockResolvedValueOnce({
      ok: true,
      data: {
        rows: [
          {
            trade_id: 'trade-1',
            ts_ms: 1772623943812,
            strategy_id: 'makerv3',
            instrument_id: 'PLUMEUSDT-LINEAR.BYBIT',
            side: '1',
            price: '2',
            qty: '3',
            mv: 0,
          },
        ],
        total: 1,
        limit: 50,
        offset: 0,
      },
    });

    const result = await api.getTrades(1, 50, { sort: 'ts_desc' });
    expect(result.rows).toHaveLength(1);
    expect(result.rows[0]?.mv).toBe(6);
  });

  it('normalizes tokenmm flat balances rows and prefers base_currency over UNKNOWN asset', async () => {
    setPathname('/tokenmm/balances');
    fetchJSONMock.mockResolvedValueOnce({
      ok: true,
      data: {
        rows: [
          {
            exchange: 'bybit',
            asset: 'UNKNOWN',
            base_currency: 'PLUME',
            total: '10',
            ts_ms: 1700000000000,
          },
        ],
        total: 1,
      },
    });

    const payload = await api.getBalances();
    expect(payload.rows).toHaveLength(1);
    expect(payload.rows[0]?.children[0]?.coin).toBe('PLUME');
  });
});

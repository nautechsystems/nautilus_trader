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

import { api, deriveCanonicalNaming } from './api';

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

  it('falls back to total_records when total is missing', async () => {
    fetchJSONMock.mockResolvedValueOnce({
      ok: true,
      data: {
        rows: [],
        total_records: 417,
        limit: 50,
        offset: 0,
      },
    });

    const result = await api.getTrades(1, 50, { sort: 'ts_desc' });
    expect(result.total).toBe(417);
    expect(result.total_records).toBe(417);
  });

  it('passes cursor param when present', async () => {
    await api.getTrades(1, 50, { cursor: 'abc', sort: 'ts_desc' });

    const [path] = fetchJSONMock.mock.calls[0];
    expect(path).toContain('cursor=abc');
    expect(path).toContain('offset=0');
  });

  it('passes market_type param when present', async () => {
    await api.getTrades(1, 50, { market_type: 'perp', sort: 'ts_desc' });

    const [path] = fetchJSONMock.mock.calls[0];
    expect(path).toContain('market_type=perp');
  });

  it('translates ascending sort to the backend token instead of legacy ts_asc', async () => {
    await api.getTrades(1, 50, { sort: 'ts_asc' });

    const [path] = fetchJSONMock.mock.calls[0];
    const search = (path as string).split('?')[1] ?? '';
    const params = new URLSearchParams(search);

    expect(params.get('sort')).toBe('asc');
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

  it('preserves canonical trade naming fields from backend payloads', async () => {
    fetchJSONMock.mockResolvedValueOnce({
      ok: true,
      data: {
        rows: [
          {
            trade_id: 'trade-canonical',
            ts_ms: 1772623943812,
            strategy_id: 'makerv3',
            instrument_id: 'PLUMEUSDT-LINEAR.BYBIT',
            display_name_short: 'PLUME Perp',
            display_name_long: 'Bybit PLUME Perp',
            product_type: 'perp',
            contract_type: 'linear',
            venue_root: 'bybit',
            side: '2',
            price: '0.009685',
            qty: '1000',
          },
        ],
        total: 1,
        limit: 50,
        offset: 0,
      },
    });

    const result = await api.getTrades(1, 50, { sort: 'ts_desc' });
    expect(result.rows[0]).toMatchObject({
      display_name_short: 'PLUME Perp',
      display_name_long: 'Bybit PLUME Perp',
      product_type: 'perp',
      contract_type: 'linear',
      venue_root: 'bybit',
    });
  });

  it('normalizes stale suffixed raw_symbol values into canonical naming fields', () => {
    const naming = deriveCanonicalNaming(
      {
        instrument_id: 'PLUMEUSDT-LINEAR.BYBIT',
        raw_symbol: 'PLUMEUSDT-LINEAR',
      },
      {
        exchange: 'bybit',
        symbol: 'PLUME/USDT',
        asset: 'PLUME',
      },
    );

    expect(naming).toMatchObject({
      raw_symbol: 'PLUMEUSDT',
      contract_type: 'linear',
      product_type: 'perp',
      base_asset: 'PLUME',
      quote_asset: 'USDT',
      pair: 'PLUME/USDT',
      display_name_short: 'PLUME Perp',
    });
  });

  it('derives stripped raw_symbol and alias exchange for instrument-only trade rows', async () => {
    fetchJSONMock.mockResolvedValueOnce({
      ok: true,
      data: {
        rows: [
          {
            trade_id: 'trade-derived',
            ts_ms: 1772623943812,
            strategy_id: 'makerv3',
            instrument_id: 'PLUMEUSDT.BINANCE_SPOT',
            side: '1',
            price: '0.009685',
            qty: '1000',
          },
        ],
        total: 1,
        limit: 50,
        offset: 0,
      },
    });

    const result = await api.getTrades(1, 50, { sort: 'ts_desc' });
    expect(result.rows[0]).toMatchObject({
      exchange: 'binance_spot',
      raw_symbol: 'PLUMEUSDT',
      product_type: 'spot',
      display_name_short: 'PLUME Spot',
      base_asset: 'PLUME',
      quote_asset: 'USDT',
      pair: 'PLUME/USDT',
    });
  });

  it('derives stripped raw_symbol and perp contract type for instrument-only bybit perp rows', async () => {
    fetchJSONMock.mockResolvedValueOnce({
      ok: true,
      data: {
        rows: [
          {
            trade_id: 'trade-derived-perp',
            ts_ms: 1772623943812,
            strategy_id: 'makerv3',
            instrument_id: 'PLUMEUSDT-LINEAR.BYBIT',
            side: '2',
            price: '0.009685',
            qty: '1000',
          },
        ],
        total: 1,
        limit: 50,
        offset: 0,
      },
    });

    const result = await api.getTrades(1, 50, { sort: 'ts_desc' });
    expect(result.rows[0]).toMatchObject({
      exchange: 'bybit',
      raw_symbol: 'PLUMEUSDT',
      contract_type: 'linear',
      product_type: 'perp',
      display_name_short: 'PLUME Perp',
    });
  });

  it('derives perp naming from legacy _LINEAR venues without instrument_id', () => {
    const naming = deriveCanonicalNaming(
      {
        venue: 'BYBIT_LINEAR',
        raw_symbol: 'PLUMEUSDT',
      },
      {
        exchange: 'bybit_linear',
        asset: 'PLUME',
      },
    );

    expect(naming).toMatchObject({
      venue: 'BYBIT_LINEAR',
      venue_root: 'bybit',
      contract_type: 'linear',
      product_type: 'perp',
      market_type: 'perp',
      display_name_short: 'PLUME Perp',
    });
  });

  it('derives perp naming from legacy _SWAP venues without instrument_id', () => {
    const naming = deriveCanonicalNaming(
      {
        venue: 'OKX_SWAP',
        raw_symbol: 'PLUME-USDT',
      },
      {
        exchange: 'okx_swap',
        asset: 'PLUME',
      },
    );

    expect(naming).toMatchObject({
      venue: 'OKX_SWAP',
      venue_root: 'okx',
      contract_type: 'swap',
      product_type: 'perp',
      market_type: 'perp',
      display_name_short: 'PLUME Perp',
    });
  });

  it('caps limit to 200 and re-bases offset using capped page size to avoid pagination gaps', async () => {
    await api.getTrades(3, 500, { sort: 'ts_desc' });

    const [path] = fetchJSONMock.mock.calls[0];
    const search = (path as string).split('?')[1] ?? '';
    const params = new URLSearchParams(search);

    expect(params.get('limit')).toBe('200');
    expect(params.get('offset')).toBe('400');
  });

  it('supports the documented trades delta timestamp fallback', async () => {
    await (api.getTradesDelta as any)({ afterMs: 1_700_000_000_000 }, 50);

    const [path] = fetchJSONMock.mock.calls[0];
    const search = (path as string).split('?')[1] ?? '';
    const params = new URLSearchParams(search);

    expect(params.get('after')).toBe('1700000000000');
    expect(params.has('since_seq')).toBe(false);
    expect(params.get('limit')).toBe('50');
  });

  it('sends replay cursor tie-breakers for tokenmm timestamp fallback', async () => {
    await (api.getTradesDelta as any)(
      {
        afterMs: 1_700_000_000_000,
        afterRowId: 'trade-b',
        afterVersion: 2,
      },
      50,
    );

    const [path] = fetchJSONMock.mock.calls[0];
    const search = (path as string).split('?')[1] ?? '';
    const params = new URLSearchParams(search);

    expect(params.get('after')).toBe('1700000000000');
    expect(params.get('after_row_id')).toBe('trade-b');
    expect(params.get('after_version')).toBe('2');
    expect(params.has('since_seq')).toBe(false);
  });
});

describe('api.patchStrategyParams', () => {
  beforeEach(() => {
    fetchJSONMock.mockReset();
    setPathname('/tokenmm/params');
  });

  it('treats HTTP 200 responses with data.errors as save failure', async () => {
    fetchJSONMock.mockResolvedValueOnce({
      ok: true,
      data: {
        success: [],
        failed: ['makerv3'],
        errors: [
          {
            strategy_id: 'makerv3',
            code: 'invalid_params_update',
            message: 'qty must be >= 0',
          },
        ],
      },
    });

    await expect(api.patchStrategyParams('makerv3', { qty: '-1' })).rejects.toThrow(/qty must be >= 0/i);
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

  it('appends profile to params request on equities routes', async () => {
    setPathname('/equities/params');
    fetchJSONMock.mockResolvedValue({ ok: true, data: [] });

    await api.getParams();

    const [path] = fetchJSONMock.mock.calls[0];
    expect(path).toContain('/api/v1/params?');
    expect(path).toContain('profile=equities');
  });

  it('appends profile to trades request on equities routes', async () => {
    setPathname('/equities/trades');
    fetchJSONMock.mockResolvedValue({
      ok: true,
      data: { rows: [], total: 0, limit: 50, offset: 0 },
    });

    await api.getTrades(1, 50, { sort: 'ts_desc' });

    const [path] = fetchJSONMock.mock.calls[0];
    expect(path).toContain('/api/v1/trades?');
    expect(path).toContain('profile=equities');
  });

  it('appends profile to balances request on equities routes', async () => {
    setPathname('/equities/balances');
    fetchJSONMock.mockResolvedValue({
      ok: true,
      data: { rows: [], total: 0 },
    });

    await api.getBalances();

    const [path] = fetchJSONMock.mock.calls[0];
    expect(path).toContain('/api/v1/balances?');
    expect(path).toContain('profile=equities');
  });

  it('appends profile to alerts request on equities routes', async () => {
    setPathname('/equities/alerts');
    fetchJSONMock.mockResolvedValue({
      ok: true,
      data: { rows: [], total: 0, limit: 25, offset: 0 },
    });

    await api.getAlerts();

    const [path] = fetchJSONMock.mock.calls[0];
    expect(path).toContain('/api/v1/alerts?');
    expect(path).toContain('profile=equities');
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
                display_name_short: 'PLUME Perp',
                display_name_long: 'Bybit PLUME Perp',
                product_type: 'perp',
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
      display_name_short: 'PLUME Perp',
      display_name_long: 'Bybit PLUME Perp',
      product_type: 'perp',
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
      label: 'bot_on',
      type: 'select',
      options: [['0', 'Off (0)'], ['1', 'On (1)']],
    });
    expect(schema.params.qty).toMatchObject({
      key: 'qty',
      label: 'qty',
      type: 'float',
      min_value: 0,
      max_value: 1000,
    });
  });

  it('preserves signed bid edge bounds from Flux params schema for the Params UI', async () => {
    setPathname('/tokenmm/params');
    fetchJSONMock.mockResolvedValueOnce({
      ok: true,
      data: {
        params: {
          bid_edge1: {
            type: 'number',
            description: 'Band 1 bid edge in bps.',
            minimum: -100,
            maximum: 1000,
          },
        },
        deprecated: {},
      },
    });

    const schema = await api.getParamSchema();
    expect(schema.params.bid_edge1).toMatchObject({
      key: 'bid_edge1',
      label: 'bid_edge1',
      type: 'float',
      min_value: -100,
      max_value: 1000,
    });
  });

  it('defaults equities params schema normalization to MakerV3 short labels', async () => {
    setPathname('/equities/params');
    fetchJSONMock.mockResolvedValueOnce({
      ok: true,
      data: {
        params: {
          bot_on: {
            type: 'boolean',
            description: 'Enable quote publishing and management.',
          },
          bid_edge1: {
            type: 'number',
            description: 'Band 1 bid edge in bps.',
            minimum: -100,
            maximum: 1000,
          },
        },
        deprecated: {},
      },
    });

    const schema = await api.getParamSchema();
    expect(schema.params.bot_on).toMatchObject({
      key: 'bot_on',
      label: 'bot_on',
      options: [['0', 'Off (0)'], ['1', 'On (1)']],
    });
    expect(schema.params.bid_edge1).toMatchObject({
      key: 'bid_edge1',
      label: 'bid_edge1',
      min_value: -100,
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

  it('does not derive params running flag from bot_on when backend omits running', async () => {
    setPathname('/tokenmm/params');
    fetchJSONMock.mockResolvedValueOnce({
      ok: true,
      data: [
        {
          strategy_id: 'run_on',
          params: {
            bot_on: true,
          },
        },
        {
          strategy_id: 'run_off',
          params: {
            bot_on: 0,
          },
        },
      ],
    });

    const rows = await api.getParams();
    expect(rows).toHaveLength(2);
    expect(rows[0].strategy_id).toBe('run_on');
    expect(rows[0].running).toBeNull();
    expect(rows[1].strategy_id).toBe('run_off');
    expect(rows[1].running).toBeNull();
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

  it('derives base coin from slash symbols in trade payloads', async () => {
    fetchJSONMock.mockResolvedValueOnce({
      ok: true,
      data: {
        rows: [
          {
            trade_id: 'trade-slash',
            ts_ms: 1772623943812,
            strategy_id: 'makerv3',
            symbol: 'ABC/USDT',
            side: '1',
            price: '2',
            qty: '3',
          },
        ],
        total: 1,
        limit: 50,
        offset: 0,
      },
    });

    const result = await api.getTrades(1, 50, { sort: 'ts_desc' });
    expect(result.rows).toHaveLength(1);
    expect(result.rows[0]?.coin).toBe('ABC');
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
                display_name_short: 'PLUME Spot',
                display_name_long: 'Bybit PLUME Spot',
                product_type: 'spot',
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
    expect(payload.rows[0]?.children[0]?.display_name_short).toBe('PLUME Spot');
    expect(payload.rows[0]?.children[0]?.product_type).toBe('spot');
  });

  it('normalizes equities flat balances rows with nanosecond timestamps', async () => {
    setPathname('/equities/balances');
    fetchJSONMock.mockResolvedValueOnce({
      ok: true,
      data: {
        rows: [
          {
            exchange: 'ibkr',
            coin: 'TSLA',
            total: '3',
            mv_raw: 1424.18500005,
            mark_raw: 474.72833335,
            ts_ms: 1773338848623000000,
          },
        ],
        total: 1,
        totals: {
          mv_raw: 1424.18500005,
          mv_display: '$1424.19',
        },
      },
    });

    const payload = await api.getBalances();
    expect(payload.rows).toHaveLength(1);
    expect(payload.rows[0]?.canonical).toBe('TSLA');
    expect(payload.rows[0]?.children[0]?.last_ts).toBe(1773338848623);
    expect(payload.rows[0]?.children[0]?.time_iso).toBe('2026-03-12T18:07:28.623Z');
  });

  it('normalizes equities IBKR stock balances and keeps master-equity totals', async () => {
    setPathname('/equities/balances');
    fetchJSONMock.mockResolvedValueOnce({
      ok: true,
      data: {
        rows: [
          {
            exchange: 'ibkr',
            kind: 'position',
            instrument_id: 'AAPL.NASDAQ',
            asset: 'AAPL',
            quantity: '5',
            signed_qty: '5',
            product_type: 'spot',
            contract_type: 'equity',
            display_name_short: 'AAPL Stock',
            display_name_long: 'Ibkr AAPL Stock',
            mv_raw: 1278.5,
            mark_raw: 255.7,
            ts_ms: 1773338848623,
          },
        ],
        total: 1,
        totals: {
          mv_raw: 1278.5,
          mv_display: '$1278.50',
          account_equity_raw: 7478.386872,
          account_equity_display: '$7478.39',
          withdrawable_raw: 7478.386872,
          withdrawable_display: '$7478.39',
        },
      },
    });

    const payload = await api.getBalances();
    expect(payload.rows).toHaveLength(1);
    expect(payload.rows[0]?.children[0]?.product_type).toBe('spot');
    expect(payload.rows[0]?.children[0]?.contract_type).toBe('equity');
    expect(payload.rows[0]?.children[0]?.display_name_short).toBe('AAPL Stock');
    expect((payload.totals as any).account_equity_raw).toBe(7478.386872);
    expect((payload.totals as any).withdrawable_raw).toBe(7478.386872);
  });

  it('normalizes hyperliquid xyz shared account balances with perp positions', async () => {
    setPathname('/equities/balances');
    fetchJSONMock.mockResolvedValueOnce({
      ok: true,
      data: {
        rows: [
          {
            row_id: 'equities:shared:hyperliquid.xyz.main:cash:hyperliquid:HYPERLIQUID-master:USDE',
            exchange: 'hyperliquid',
            account: 'HYPERLIQUID-master',
            asset: 'USDE',
            total: '1075.37415731',
            product_type: 'spot',
            contract_type: 'cash',
            ts_ms: 1773338848623,
            source_scope: 'shared_account',
            account_scope_id: 'hyperliquid.xyz.main',
            strategy_id: 'equities',
          },
          {
            row_id: 'equities:shared:hyperliquid.xyz.main:pos:hyperliquid:HYPERLIQUID-master:xyz:NVDA-USD-PERP.HYPERLIQUID',
            exchange: 'hyperliquid',
            kind: 'position',
            instrument_id: 'xyz:NVDA-USD-PERP.HYPERLIQUID',
            asset: 'NVDA',
            signed_qty: '-9.111',
            quantity: '9.111',
            product_type: 'perp',
            contract_type: 'perp',
            mark_raw: 183.22,
            mv_raw: -1669.32042,
            ts_ms: 1773338848624,
            source_scope: 'shared_account',
            account_scope_id: 'hyperliquid.xyz.main',
            strategy_id: 'equities',
          },
          {
            row_id: 'equities:shared:hyperliquid.xyz.main:pos:hyperliquid:HYPERLIQUID-master:xyz:COIN-USD-PERP.HYPERLIQUID',
            exchange: 'hyperliquid',
            kind: 'position',
            instrument_id: 'xyz:COIN-USD-PERP.HYPERLIQUID',
            asset: 'COIN',
            signed_qty: '-22.715',
            quantity: '22.715',
            product_type: 'perp',
            contract_type: 'perp',
            mark_raw: 194.5,
            mv_raw: -4418.0675,
            ts_ms: 1773338848625,
            source_scope: 'shared_account',
            account_scope_id: 'hyperliquid.xyz.main',
            strategy_id: 'equities',
          },
          {
            row_id: 'equities:shared:hyperliquid.xyz.main:pos:hyperliquid:HYPERLIQUID-master:xyz:GOOGL-USD-PERP.HYPERLIQUID',
            exchange: 'hyperliquid',
            kind: 'position',
            instrument_id: 'xyz:GOOGL-USD-PERP.HYPERLIQUID',
            asset: 'GOOGL',
            signed_qty: '-6',
            quantity: '6',
            product_type: 'perp',
            contract_type: 'perp',
            mark_raw: 303.15,
            mv_raw: -1818.9,
            ts_ms: 1773338848626,
            source_scope: 'shared_account',
            account_scope_id: 'hyperliquid.xyz.main',
            strategy_id: 'equities',
          },
        ],
        total: 4,
        totals: {
          mv_raw: -6830.91342,
          mv_display: '-$6830.91',
          account_equity_raw: 8314.466609,
          account_equity_display: '$8314.47',
          withdrawable_raw: 0,
          withdrawable_display: '$0.00',
        },
      },
    });

    const payload = await api.getBalances();
    const byCanonical = Object.fromEntries(payload.rows.map((row) => [row.canonical, row]));

    expect(Object.keys(byCanonical).sort()).toEqual(['COIN', 'GOOGL', 'NVDA', 'USDE']);
    expect(
      payload.rows
        .filter((row) => row.canonical !== 'USDE')
        .every((row) => row.children.every((child) => child.contract_type === 'perp')),
    ).toBe(true);
    expect(byCanonical.NVDA?.children[0]?.product_type).toBe('perp');
    expect(byCanonical.NVDA?.children[0]?.contract_type).toBe('perp');
    expect(byCanonical.NVDA?.children[0]?.display_name_short).toBe('NVDA Perp');
    expect(byCanonical.COIN?.children[0]?.instrument_id).toBe('XYZ:COIN-USD-PERP.HYPERLIQUID');
    expect(byCanonical.GOOGL?.children[0]?.qty_raw).toBe(-6);
    expect(byCanonical.USDE?.children[0]?.contract_type).toBe('cash');
    expect((payload.totals as any).account_equity_raw).toBe(8314.466609);
    expect((payload.totals as any).withdrawable_raw).toBe(0);
  });

  it('preserves alerts pagination metadata on getAlerts while keeping array return shape', async () => {
    fetchJSONMock.mockResolvedValueOnce({
      ok: true,
      data: {
        rows: [
          {
            id: 'alert-1',
            level: 'WARNING',
            message: 'needs attention',
            details: {},
            timestamp: 1700000000,
          },
        ],
        total: 10,
        limit: 1,
        offset: 0,
        has_more: true,
        next_offset: 1,
        next_cursor: 'alerts-cursor-1',
      },
    });

    const alerts = await api.getAlerts();
    expect(Array.isArray(alerts)).toBe(true);
    expect(alerts).toHaveLength(1);
    expect((alerts as any).total).toBe(10);
    expect((alerts as any).limit).toBe(1);
    expect((alerts as any).offset).toBe(0);
    expect((alerts as any).has_more).toBe(true);
    expect((alerts as any).next_offset).toBe(1);
    expect((alerts as any).next_cursor).toBe('alerts-cursor-1');
  });

  it('normalizes alerts rows with row_id fallback and default fields', async () => {
    fetchJSONMock.mockResolvedValueOnce({
      ok: true,
      data: {
        rows: [
          {
            row_id: 'alert-row-2',
            severity: 'warning',
            title: 'No explicit message',
            details: null,
          },
        ],
      },
    });

    const alerts = await api.getAlerts();
    expect(alerts).toHaveLength(1);
    expect(alerts[0]?.id).toBe('alert-row-2');
    expect(alerts[0]?.level).toBe('WARNING');
    expect(alerts[0]?.message).toBe('No explicit message');
    expect(alerts[0]?.details).toEqual({});
    expect(typeof alerts[0]?.timestamp).toBe('number');
    expect((alerts[0]?.timestamp ?? 0) > 0).toBe(true);
  });

  it('normalizes alerts rows using ts_ms and strategy fallback fields', async () => {
    fetchJSONMock.mockResolvedValueOnce({
      ok: true,
      data: {
        rows: [
          {
            row_id: 'alert-row-ts-ms',
            level: 'info',
            ts_ms: 1_700_000_111_222,
            strategy: 'strategy_from_alt_field',
            title: 'fallback strategy mapping',
          },
        ],
      },
    });

    const alerts = await api.getAlerts();
    expect(alerts).toHaveLength(1);
    expect(alerts[0]?.id).toBe('alert-row-ts-ms');
    expect(alerts[0]?.timestamp).toBe(1_700_000_111);
    expect((alerts[0] as any)?.strategy_id).toBe('strategy_from_alt_field');
  });

  it('normalizes alerts rows using entry_id fallback identity and preserves ERROR severity', async () => {
    fetchJSONMock.mockResolvedValueOnce({
      ok: true,
      data: {
        rows: [
          {
            entry_id: '1700000000001-0',
            level: 'error',
            message: 'borrow denied',
            ts_ms: 1_700_000_111_222,
            strategy_id: 'strategy_error_case',
          },
        ],
      },
    });

    const alerts = await api.getAlerts();
    expect(alerts).toHaveLength(1);
    expect(alerts[0]?.id).toBe('1700000000001-0');
    expect(alerts[0]?.level).toBe('ERROR');
    expect((alerts[0] as any)?.strategy_id).toBe('strategy_error_case');
  });

  it('supports alerts payloads using data.alerts in addition to data.rows', async () => {
    fetchJSONMock.mockResolvedValueOnce({
      ok: true,
      data: {
        alerts: [
          {
            row_id: 'alert-from-alerts-key',
            severity: 'critical',
            message: 'critical alert',
            details: {},
            timestamp: 1_700_000_000,
          },
        ],
      },
    });

    const alerts = await api.getAlerts();
    expect(alerts).toHaveLength(1);
    expect(alerts[0]?.id).toBe('alert-from-alerts-key');
    expect(alerts[0]?.level).toBe('CRITICAL');
  });
});

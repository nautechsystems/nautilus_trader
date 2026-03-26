import { expect, test } from '@playwright/test';

type TradeRowFixture = {
  row_id: string;
  version: number;
  seq: number;
  ts: number;
  time: string;
  coin: string;
  exchange: string;
  side: string;
  price: number;
  qty: number;
  mv: number;
  fee: number;
  trade_id: string;
  exch_id: string;
  order_id: string;
  signal_id: string;
  stream_id?: string;
  snapshot_revision?: string;
};

type AlertFixture = {
  id: string;
  level: 'INFO' | 'WARNING' | 'ERROR' | 'CRITICAL';
  severity?: 'INFO' | 'WARNING' | 'ERROR' | 'CRITICAL';
  title: string;
  message: string;
  timestamp: number;
  ts?: number;
  details?: Record<string, unknown>;
};

type MarketRowFixture = {
  coin: string;
  exchange: string;
  bid: string;
  bid_qty: string;
  mid_px: string;
  ask: string;
  ask_qty: string;
  timestamp_ms: number;
};

const REALTIME_FLAGS = [
  'fluxboard:feature:realtime-standard',
  'fluxboard:feature:realtime-standard-signal',
  'fluxboard:feature:realtime-standard-trades',
  'fluxboard:feature:realtime-standard-alerts',
  'fluxboard:feature:realtime-standard-balances',
  'fluxboard:feature:realtime-standard-marketdata',
] as const;

const REALTIME_BUDGETS = {
  maxMountedRows: 120,
} as const;

const DASHBOARD_INVALIDATION_EVENTS = 50;
const MARKET_DATA_INVALIDATION_EVENTS = 50;

function createDashboardLayout() {
  const lg = [
    { i: 'signal', x: 0, y: 0, w: 12, h: 6 },
    { i: 'trades', x: 0, y: 6, w: 12, h: 5 },
    { i: 'alerts', x: 0, y: 11, w: 6, h: 4 },
    { i: 'balances', x: 6, y: 11, w: 6, h: 4 },
  ];
  return {
    version: 2,
    preset: 'default',
    layouts: {
      lg,
      md: lg,
      sm: lg,
      xs: lg,
      xxs: lg,
    },
  };
}

async function installTestRuntime(page: Parameters<typeof test>[0]['page']) {
  await page.addInitScript(({ flags, layout }) => {
    window.localStorage.clear();
    for (const flag of flags) {
      window.localStorage.setItem(flag, 'true');
    }
    window.localStorage.setItem('fluxboard:dashboard:layout:default', JSON.stringify(layout));
    window.localStorage.setItem('fluxboard:dashboard:collapsed', JSON.stringify([]));

    const listeners = new Map<string, Set<(payload?: any) => void>>();
    const getBucket = (event: string) => {
      let bucket = listeners.get(event);
      if (!bucket) {
        bucket = new Set();
        listeners.set(event, bucket);
      }
      return bucket;
    };
    const emit = (event: string, payload?: any) => {
      for (const handler of listeners.get(event) ?? []) {
        handler(payload);
      }
    };

    const testSocket: any = {
      connected: true,
      id: 'pw-realtime-soak-socket',
      io: {
        reconnect: () => {},
        engine: {
          transport: {
            close: () => {},
          },
        },
      },
      on(event: string, handler: (payload?: any) => void) {
        getBucket(event).add(handler);
        return testSocket;
      },
      off(event: string, handler?: (payload?: any) => void) {
        if (!handler) {
          listeners.delete(event);
          return testSocket;
        }
        listeners.get(event)?.delete(handler);
        return testSocket;
      },
      emit(event: string, payload?: any) {
        if (event === 'set_profile') {
          testSocket.profile = payload?.profile;
          return true;
        }
        emit(event, payload);
        return true;
      },
      connect() {
        if (!testSocket.connected) {
          testSocket.connected = true;
          emit('connect');
        }
        return testSocket;
      },
      disconnect() {
        if (testSocket.connected) {
          testSocket.connected = false;
          emit('disconnect', 'io client disconnect');
        }
        return testSocket;
      },
      removeAllListeners() {
        listeners.clear();
        return testSocket;
      },
      __emitServer(event: string, payload?: any) {
        emit(event, payload);
      },
    };

    (window as any).__fluxboardTestSocket = testSocket;
    (window as any).__fluxboardTestSocketFactory = () => testSocket;
  }, { flags: [...REALTIME_FLAGS], layout: createDashboardLayout() });
}

function makeTradeRow(overrides: Partial<TradeRowFixture> = {}): TradeRowFixture {
  return {
    row_id: 'trade-row',
    version: 1,
    seq: 1,
    ts: 1,
    time: '2025-01-01T00:00:01.000Z',
    coin: 'ALPHA',
    exchange: 'bybit',
    side: 'buy',
    price: 101,
    qty: 1,
    mv: 101,
    fee: 0.1,
    trade_id: 'trade-1',
    exch_id: 'exec-1',
    order_id: 'order-1',
    signal_id: 'signal-1',
    ...overrides,
  };
}

function makeSignalStrategy(index: number, edgeShift = 0) {
  const id = `signal-${String(index).padStart(3, '0')}`;
  return {
    id,
    params: {
      bot_on: index % 2 === 0 ? '1' : '0',
      cex_bid_edge: '10',
      cex_ask_edge: '10',
      pool_edge: '10',
      qty: String(100 + index),
      slippage_bps: '50',
    },
    legs: {
      A: {
        exchange: 'bybit',
        coin: 'PLUME',
        decision_bid: 1.0 + edgeShift,
        decision_ask: 1.01 + edgeShift,
        net_edge_bps: 10 + edgeShift,
        update_time: '2026-03-23 00:00:00',
      },
      B: {
        exchange: 'rooster',
        coin: 'WPLUME',
        decision_bid: 1.02 + edgeShift,
        decision_ask: 1.03 + edgeShift,
        net_edge_bps: 12 + edgeShift,
        update_time: '2026-03-23 00:00:00',
      },
    },
    balances_ok: true,
    edge2_bps: 5 + edgeShift,
    risk_delta: 25 + index,
  };
}

function buildSignalStrategies(count: number, edgeShift = 0) {
  return Array.from({ length: count }, (_, index) => makeSignalStrategy(index, edgeShift));
}

function makeAlert(overrides: Partial<AlertFixture> = {}): AlertFixture {
  return {
    id: 'alert-1',
    level: 'WARNING',
    severity: 'WARNING',
    title: 'Spread drift',
    message: 'Spread drift widened',
    timestamp: 1_700_000_000,
    ts: 1_700_000_000,
    details: {},
    ...overrides,
  };
}

function makeMarketRow(overrides: Partial<MarketRowFixture> = {}): MarketRowFixture {
  return {
    coin: 'BTC/USDT',
    exchange: 'bybit',
    bid: '100',
    bid_qty: '1',
    mid_px: '101',
    ask: '102',
    ask_qty: '2',
    timestamp_ms: 1_700_000_000_000,
    ...overrides,
  };
}

function makeBalancePayload({
  qty = 1_500,
  mv = 75.5,
  withdrawable = 7_478.39,
}: {
  qty?: number;
  mv?: number;
  withdrawable?: number;
} = {}) {
  const parentId = 'PLUME_LOGICAL';
  const childId = `${parentId}:PLUME:wallet_primary`;
  const mark = qty > 0 ? mv / qty : mv;
  return {
    data: {
      rows: [
        {
          id: parentId,
          coin: parentId,
          canonical: 'PLUME',
          is_parent: true as const,
          stable: false,
          qty_display: qty.toLocaleString('en-US'),
          qty_raw: qty,
          mv_display: `$${mv.toFixed(2)}`,
          mv_raw: mv,
          mark_display: mark.toFixed(4),
          mark_raw: mark,
          time_display: '1m ago',
          time_iso: '2026-03-23T00:00:00Z',
          last_ts: 1_763_000_000_000,
          raw: { qty, mv_usd: mv, mark },
          children: [
            {
              id: childId,
              parent_id: parentId,
              coin: 'PLUME',
              display_name_short: 'PLUME Spot',
              display_name_long: 'PLUME Spot Wallet',
              product_type: 'spot',
              inventory_asset: 'PLUME',
              venue: 'wallet',
              wallet: 'treasury',
              address: '0xabc1234567890000000000000000000000000000',
              label: 'Treasury',
              contract: 'PLUMEUSDT',
              qty_display: qty.toLocaleString('en-US'),
              qty_raw: qty,
              mv_display: `$${mv.toFixed(2)}`,
              mv_raw: mv,
              mark_display: mark.toFixed(4),
              mark_raw: mark,
              time_display: '1m ago',
              time_iso: '2026-03-23T00:00:00Z',
              last_ts: 1_763_000_000_000,
            },
          ],
        },
      ],
      total: 1,
      totals: {
        mv_raw: mv,
        mv_display: `$${mv.toFixed(2)}`,
        net_mv_raw: mv,
        net_mv_display: `$${mv.toFixed(2)}`,
        long_mv_raw: mv,
        long_mv_display: `$${mv.toFixed(2)}`,
        short_mv_raw: 0,
        short_mv_display: '$0.00',
        gross_mv_raw: mv,
        gross_mv_display: `$${mv.toFixed(2)}`,
        stable_mv_raw: 0,
        stable_mv_display: '$0.00',
        non_stable_mv_raw: mv,
        non_stable_mv_display: `$${mv.toFixed(2)}`,
        account_equity_raw: withdrawable,
        account_equity_display: `$${withdrawable.toFixed(2)}`,
        withdrawable_raw: withdrawable,
        withdrawable_display: `$${withdrawable.toFixed(2)}`,
      },
      generated_at: '2026-03-23T00:00:00Z',
      view: 'parents_only',
      risk_groups: [],
    },
  };
}

test.describe.configure({ mode: 'serial' });

test.describe('Realtime cleanup readiness rehearsal soak', () => {
  test('staged surface rollout stays bounded through dashboard soak and market-data recovery', async ({ page, baseURL }) => {
    const signalRequests: string[] = [];
    const tradesSnapshotRequests: string[] = [];
    const tradesDeltaRequests: string[] = [];
    const alertsRequests: string[] = [];
    const balancesRequests: string[] = [];
    const marketDataRequests: string[] = [];

    await installTestRuntime(page);

    await page.route('**/api/v1/signals*', async (route) => {
      signalRequests.push(route.request().url());
      const requestNumber = signalRequests.length;
      const strategies = buildSignalStrategies(200, requestNumber > 1 ? 1 : 0);
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          ok: true,
          data: {
            strategies,
            server_time: '2026-03-23 00:00:00',
            server_ts_ms: 1_763_000_000_000 + requestNumber,
            balance_summary: null,
          },
        }),
      });
    });

    await page.route('**/api/v1/trades?**', async (route) => {
      tradesSnapshotRequests.push(route.request().url());
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          ok: true,
          data: {
            rows: [
              makeTradeRow({
                row_id: 'old',
                seq: 1,
                ts: 1,
                time: '2025-01-01T00:00:01.000Z',
                coin: 'OLD',
                price: 101,
              }),
              makeTradeRow({
                row_id: 'new',
                seq: 2,
                ts: 2,
                time: '2025-01-01T00:00:02.000Z',
                coin: 'NEW',
                side: 'sell',
                price: 102,
              }),
            ],
            total: 2,
            limit: 100,
            offset: 0,
            page: 1,
            page_size: 100,
            last_seq: 2,
            stream_id: 'trades-main',
            snapshot_revision: 'snap-1',
            has_more: false,
            next_offset: null,
            next_cursor: null,
            sort: 'ts_desc',
          },
        }),
      });
    });

    await page.route('**/api/v1/trades/delta?**', async (route) => {
      tradesDeltaRequests.push(route.request().url());
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          ok: true,
          data: {
            rows: Array.from({ length: 8 }, (_, index) => {
              const seq = 53 + index;
              return makeTradeRow({
                row_id: `gap-${seq}`,
                seq,
                ts: seq,
                time: `2025-01-01T00:00:${String(seq).padStart(2, '0')}.000Z`,
                coin: `GAP${seq}`,
                price: 100 + seq,
              });
            }),
            last_seq: 60,
            reset_required: false,
            stream_id: 'trades-main',
            snapshot_revision: 'snap-1',
          },
        }),
      });
    });

    await page.route('**/api/v1/alerts*', async (route) => {
      alertsRequests.push(route.request().url());
      const requestNumber = alertsRequests.length;
      const rows = requestNumber === 1
        ? [
            makeAlert({
              id: 'alert-initial',
              title: 'Initial warning',
              message: 'Initial warning',
              timestamp: 1_700_000_000,
              ts: 1_700_000_000,
            }),
          ]
        : [
            makeAlert({
              id: 'alert-recovered',
              level: 'CRITICAL',
              severity: 'CRITICAL',
              title: 'Recovered alert',
              message: 'Recovered alert after summary refresh',
              timestamp: 1_700_000_100,
              ts: 1_700_000_100,
            }),
          ];

      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          ok: true,
          data: {
            rows,
            total: rows.length,
            limit: 100,
            offset: 0,
            has_more: false,
            next_offset: null,
            next_cursor: null,
          },
        }),
      });
    });

    await page.route('**/api/v1/balances*', async (route) => {
      balancesRequests.push(route.request().url());
      const payload = balancesRequests.length === 1
        ? makeBalancePayload({ qty: 1_500, mv: 75.5, withdrawable: 7_478.39 })
        : makeBalancePayload({ qty: 1_650, mv: 82.75, withdrawable: 7_500.0 });
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ ok: true, ...payload }),
      });
    });

    await page.route('**/api/v1/market-data/snapshot', async (route) => {
      marketDataRequests.push(route.request().url());
      const requestNumber = marketDataRequests.length;
      const rows = Array.from({ length: 200 }, (_, index) => makeMarketRow({
        coin: `COIN-${String(index).padStart(3, '0')}/USDT`,
        exchange: index % 2 === 0 ? 'bybit' : 'bitget',
        bid: String(100 + index),
        mid_px: String(101 + index + (requestNumber > 1 ? 1 : 0)),
        ask: String(102 + index),
        timestamp_ms: 1_700_000_000_000 + index,
      }));
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          ok: true,
          data: {
            rows,
            total: rows.length,
            limit: 50,
            offset: 0,
            has_more: true,
            next_offset: 50,
            next_cursor: null,
          },
        }),
      });
    });

    await page.goto(`${baseURL ?? ''}/dashboard`);

    const signalPanel = page.locator('.dashboard-panel[data-panel-title="Signal"]');
    const tradesPanel = page.locator('.dashboard-panel[data-panel-title="Trades"]');
    const alertsPanel = page.locator('.dashboard-panel[data-panel-title="Alerts"]');
    const balancesPanel = page.locator('.dashboard-panel[data-panel-title="Balances"]');

    await expect(signalPanel).toBeVisible();
    await expect(tradesPanel).toBeVisible();
    await expect(alertsPanel).toBeVisible();
    await expect(balancesPanel).toBeVisible();
    await expect(signalPanel.getByText('signal-000')).toBeVisible();
    await expect(alertsPanel.getByRole('cell', { name: 'Initial warning' }).first()).toBeVisible();
    await expect(balancesPanel.getByText('PLUME').first()).toBeVisible();

    await expect.poll(() => signalRequests.length, { timeout: 6_000 }).toBe(1);
    await expect.poll(() => tradesSnapshotRequests.length, { timeout: 6_000 }).toBe(1);
    expect(tradesDeltaRequests).toHaveLength(0);
    await expect.poll(() => alertsRequests.length, { timeout: 6_000 }).toBe(1);
    await expect.poll(() => balancesRequests.length, { timeout: 6_000 }).toBe(1);

    let mountedRows = await page.locator('.dashboard-panel tbody tr').count();
    expect(mountedRows).toBeLessThanOrEqual(REALTIME_BUDGETS.maxMountedRows);

    await page.evaluate((dashboardInvalidations) => {
      const socket = (window as any).__fluxboardTestSocket;
      for (let i = 0; i < dashboardInvalidations; i += 1) {
        socket.__emitServer('market_update', {
          strategies: { changed: ['signal-000'] },
          alerts: {
            count: 1,
            latest_ts_ms: 1_700_000_100_000 + i,
          },
        });
      }

      for (let seq = 3; seq <= 52; seq += 1) {
        socket.__emitServer('trade_update', {
          op: 'upsert',
          row_id: `live-${seq}`,
          version: 1,
          seq,
          ts: seq,
          time: `2025-01-01T00:00:${String(seq).padStart(2, '0')}.000Z`,
          coin: `LIVE${seq}`,
          exchange: 'bybit',
          side: seq % 2 === 0 ? 'buy' : 'sell',
          price: 100 + seq,
          qty: 1,
          mv: 100 + seq,
          fee: 0.1,
          trade_id: `trade-${seq}`,
          exch_id: `exec-${seq}`,
          order_id: `order-${seq}`,
          signal_id: 'signal-1',
          stream_id: 'trades-main',
          snapshot_revision: 'snap-1',
        });
      }

      socket.__emitServer('trade_update', {
        op: 'upsert',
        row_id: 'gap-60',
        version: 1,
        seq: 60,
        ts: 60,
        time: '2025-01-01T00:01:00.000Z',
        coin: 'GAP60',
        exchange: 'bybit',
        side: 'buy',
        price: 160,
        qty: 1,
        mv: 160,
        fee: 0.1,
        trade_id: 'trade-60',
        exch_id: 'exec-60',
        order_id: 'order-60',
        signal_id: 'signal-1',
        stream_id: 'trades-main',
        snapshot_revision: 'snap-1',
      });
    }, DASHBOARD_INVALIDATION_EVENTS);

    await expect.poll(() => signalRequests.length, { timeout: 6_000 }).toBeGreaterThanOrEqual(2);
    await expect.poll(() => alertsRequests.length, { timeout: 6_000 }).toBeGreaterThanOrEqual(2);
    await expect.poll(() => balancesRequests.length, { timeout: 6_000 }).toBeGreaterThanOrEqual(2);
    await expect.poll(() => tradesDeltaRequests.length, { timeout: 6_000 }).toBe(1);

    expect(signalRequests.length).toBeLessThanOrEqual(3);
    expect(alertsRequests.length).toBeLessThanOrEqual(3);
    expect(balancesRequests.length).toBeLessThanOrEqual(3);

    const replayQuery = new URL(tradesDeltaRequests[0]);
    expect(replayQuery.searchParams.get('since_seq')).toBe('52');
    expect(replayQuery.searchParams.get('stream_id')).toBe('trades-main');
    expect(replayQuery.searchParams.get('snapshot_revision')).toBe('snap-1');
    expect(replayQuery.searchParams.get('after')).toBeNull();

    await expect(alertsPanel.getByText('Recovered alert after summary refresh')).toBeVisible();
    await expect(
      balancesPanel.getByRole('row', { name: /PLUME Logical 1,650 \$82\.75 0\.0502/ }),
    ).toBeVisible();
    await expect(tradesPanel.getByText('GAP60')).toBeVisible();

    const signalToggle = signalPanel.locator('button[title*="Collapse"]').or(
      signalPanel.locator('button[title*="Expand"]'),
    );
    await expect(signalToggle).toBeVisible();
    await signalToggle.click({ force: true });
    await signalToggle.click({ force: true });
    await expect(signalPanel.locator('[data-testid="panel-body"]')).toHaveCount(1);

    mountedRows = await page.locator('.dashboard-panel tbody tr').count();
    expect(mountedRows).toBeLessThanOrEqual(REALTIME_BUDGETS.maxMountedRows);

    await page.goto(`${baseURL ?? ''}/market-data`);
    await expect(
      page.getByRole('row', { name: /COIN-199\/USDT bitget 299 300 301/ }),
    ).toBeVisible();

    await expect.poll(() => marketDataRequests.length, { timeout: 6_000 }).toBe(1);

    await page.evaluate((marketDataInvalidations) => {
      const socket = (window as any).__fluxboardTestSocket;
      for (let i = 0; i < marketDataInvalidations; i += 1) {
        socket.__emitServer('market_update', {
          venue: 'bybit',
          latest_ts_ms: 1_700_000_200_000 + i,
        });
      }
    }, MARKET_DATA_INVALIDATION_EVENTS);

    await expect.poll(() => marketDataRequests.length, { timeout: 6_000 }).toBeGreaterThanOrEqual(2);
    expect(marketDataRequests.length).toBeLessThanOrEqual(3);
    await expect(
      page.getByRole('row', { name: /COIN-199\/USDT bitget 299 301 301/ }),
    ).toBeVisible();

  });
});

import { expect, test } from '@playwright/test';

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

type BalanceChildFixture = {
  id: string;
  parent_id: string;
  coin: string;
  display_name_short?: string;
  display_name_long?: string;
  product_type?: string;
  inventory_asset?: string;
  venue: string;
  wallet?: string | null;
  address?: string | null;
  label?: string | null;
  contract?: string;
  qty_display: string;
  qty_raw: number;
  mv_display: string;
  mv_raw: number;
  mark_display: string;
  mark_raw: number;
  time_display: string;
  time_iso: string;
  last_ts: number;
};

type BalanceParentFixture = {
  id: string;
  coin: string;
  canonical: string;
  is_parent: true;
  stable: boolean;
  qty_display: string;
  qty_raw: number;
  mv_display: string;
  mv_raw: number;
  mark_display: string;
  mark_raw: number;
  time_display: string;
  time_iso: string;
  last_ts: number;
  raw: Record<string, unknown>;
  children: BalanceChildFixture[];
};

const installTestSocket = async (
  page: Parameters<typeof test>[0]['page'],
  flags: string[],
) => {
  await page.addInitScript((storageFlags: string[]) => {
    window.localStorage.clear();
    for (const flag of storageFlags) {
      window.localStorage.setItem(flag, 'true');
    }

    const listeners = new Map<string, Set<(payload?: any) => void>>();
    const socketEmits: Array<{ event: string; payload: any }> = [];
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
      id: 'pw-market-balances-socket',
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
      emit(event: string, payload?: any, ack?: (response: any) => void) {
        socketEmits.push({ event, payload });
        if (event === 'set_profile') {
          testSocket.profile = payload?.profile;
          return true;
        }
        if (event === 'subscribe' && typeof ack === 'function') {
          ack({
            accepted: true,
            contract_version: payload?.contract_version,
            surface: payload?.surface,
            profile: payload?.profile,
            surface_query_key: payload?.surface_query_key,
            stream_id: payload?.stream_id,
            snapshot_revision: payload?.snapshot_revision,
            accepted_start_seq: payload?.resume_from_seq,
            last_seq: payload?.resume_from_seq,
            requested_resume_from_seq: payload?.resume_from_seq,
            capabilities: {
              recovery_mode: 'invalidate_only',
              replay_supported: false,
              transport_mode: 'polling_only',
            },
          });
          return true;
        }
        if (event === 'unsubscribe' && typeof ack === 'function') {
          ack({ ok: true, surface: payload?.surface ?? null });
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
    (window as any).__fluxboardSocketEmits = socketEmits;
  }, flags);
};

const makeMarketRow = (overrides: Partial<MarketRowFixture> = {}): MarketRowFixture => ({
  coin: 'BTC/USDT',
  exchange: 'bybit',
  bid: '100',
  bid_qty: '1',
  mid_px: '101',
  ask: '102',
  ask_qty: '2',
  timestamp_ms: 1_700_000_000_000,
  ...overrides,
});

const makeBalancePayload = ({
  canonical = 'PLUME',
  childCoin = 'PLUME',
  qty = 1_500,
  mv = 75.5,
  withdrawable = 7_478.39,
  stable = false,
}: {
  canonical?: string;
  childCoin?: string;
  qty?: number;
  mv?: number;
  withdrawable?: number;
  stable?: boolean;
} = {}) => {
  const upperCanonical = canonical.toUpperCase();
  const upperChildCoin = childCoin.toUpperCase();
  const parentId = `${upperCanonical}_LOGICAL`;
  const childId = `${parentId}:${upperChildCoin}:wallet_primary`;
  const mark = qty > 0 ? mv / qty : mv;
  return {
    data: {
      rows: [
        {
          id: parentId,
          coin: parentId,
          canonical: upperCanonical,
          is_parent: true as const,
          stable,
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
              coin: upperChildCoin,
              display_name_short: `${upperChildCoin} Spot`,
              display_name_long: `${upperChildCoin} Spot Wallet`,
              product_type: 'spot',
              inventory_asset: upperChildCoin,
              venue: 'wallet',
              wallet: 'treasury',
              address: '0xabc1234567890000000000000000000000000000',
              label: 'Treasury',
              contract: `${upperChildCoin}USDT`,
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
        } satisfies BalanceParentFixture,
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
        stable_mv_raw: stable ? mv : 0,
        stable_mv_display: stable ? `$${mv.toFixed(2)}` : '$0.00',
        non_stable_mv_raw: stable ? 0 : mv,
        non_stable_mv_display: stable ? '$0.00' : `$${mv.toFixed(2)}`,
        account_equity_raw: withdrawable,
        account_equity_display: `$${withdrawable.toFixed(2)}`,
        withdrawable_raw: withdrawable,
        withdrawable_display: `$${withdrawable.toFixed(2)}`,
      },
      generated_at: '2026-03-23T00:00:00Z',
      view: 'parents_only',
      risk_groups: [],
      realtime: {
        contract_version: 2,
        surface: 'balances',
        profile: 'tokenmm',
        surface_query_key: 'balances|profile=tokenmm|strategy_ids=strategy_01',
        stream_id: 'balances:tokenmm:strategy_01',
        snapshot_revision: 'balances-snap-1',
        last_seq: stable ? 1 : 0,
        capabilities: {
          recovery_mode: 'invalidate_only',
          replay_supported: false,
          transport_mode: 'polling_only',
        },
      },
    },
  };
};

test.describe('MarketData and Balances realtime cutover', () => {
  test('MarketData stays idle while healthy and refreshes once on market_update when the standard surface is enabled', async ({ page, baseURL }) => {
    const snapshotRequests: string[] = [];
    await installTestSocket(page, [
      'fluxboard:feature:realtime-standard',
      'fluxboard:feature:realtime-standard-marketdata',
    ]);

    await page.route('**/api/v1/market-data/snapshot', async (route) => {
      snapshotRequests.push(route.request().url());
      const rows = snapshotRequests.length === 1
        ? [
            makeMarketRow({
              coin: 'BTC/USDT',
              exchange: 'bybit',
              bid: '100',
              ask: '102',
              mid_px: '101',
            }),
          ]
        : [
            makeMarketRow({
              coin: 'ETH/USDT',
              exchange: 'binance',
              bid: '200',
              ask: '202',
              mid_px: '201',
              timestamp_ms: 1_700_000_005_000,
            }),
          ];

      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          ok: true,
          data: {
            rows,
            count: rows.length,
            freshness_key: `market-snapshot-${snapshotRequests.length}`,
            last_update_ms: 1_700_000_000_000 + (snapshotRequests.length * 5_000),
          },
        }),
      });
    });

    await page.goto(`${baseURL ?? ''}/market-data`);

    await expect(page.getByText('BTC/USDT')).toBeVisible();
    await expect(page.getByText('100')).toBeVisible();
    await page.waitForTimeout(5_200);
    expect(snapshotRequests).toHaveLength(1);

    await page.evaluate(() => {
      (window as any).__fluxboardTestSocket.__emitServer('market_update', {
        marketData: { reason: 'test-refresh' },
      });
    });

    await expect(page.getByText('ETH/USDT')).toBeVisible();
    await expect(page.getByText('200')).toBeVisible();
    await expect(page.getByText('BTC/USDT')).toHaveCount(0);
    expect(snapshotRequests).toHaveLength(2);
  });

  test('Balances uses standard subscribe and invalidation recovery when the standard surface is enabled', async ({ page, baseURL }) => {
    const balanceRequests: string[] = [];
    await installTestSocket(page, [
      'fluxboard:feature:realtime-standard',
      'fluxboard:feature:realtime-standard-balances',
    ]);

    await page.route('**/api/v1/balances*', async (route) => {
      balanceRequests.push(route.request().url());
      const payload = balanceRequests.length === 1
        ? makeBalancePayload({
            canonical: 'PLUME',
            childCoin: 'PLUME',
            qty: 1_500,
            mv: 75.5,
            withdrawable: 7_478.39,
          })
        : makeBalancePayload({
            canonical: 'USDC',
            childCoin: 'USDC',
            qty: 2_000,
            mv: 2_000,
            withdrawable: 9_999.99,
            stable: true,
          });

      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(payload),
      });
    });

    await page.goto(`${baseURL ?? ''}/balances`);

    await expect(page.getByText('PLUME', { exact: true })).toBeVisible();
    await expect(page.getByText('Net Equity (Σ MV): $75.50')).toBeVisible();
    await expect(page.getByText('Account Equity')).toBeVisible();
    const balanceSubscribe = await page.evaluate(() => {
      const emits = (window as any).__fluxboardSocketEmits as Array<{ event: string; payload: any }>;
      return emits.find((entry) => entry.event === 'subscribe' && entry.payload?.surface === 'balances')?.payload ?? null;
    });
    expect(balanceSubscribe).toMatchObject({
      contract_version: 2,
      surface: 'balances',
      profile: 'tokenmm',
      surface_query_key: 'balances|profile=tokenmm|strategy_ids=strategy_01',
      stream_id: 'balances:tokenmm:strategy_01',
      snapshot_revision: 'balances-snap-1',
      resume_from_seq: 0,
    });

    const requestsAtHealthyIdleStart = balanceRequests.length;
    await page.waitForTimeout(5_200);
    expect(balanceRequests).toHaveLength(requestsAtHealthyIdleStart);

    await page.evaluate(() => {
      (window as any).__fluxboardTestSocket.__emitServer('realtime_event', {
        contract_version: 2,
        surface: 'balances',
        profile: 'tokenmm',
        stream_id: 'balances:tokenmm:strategy_01',
        kind: 'invalidate',
        seq: 1,
        snapshot_revision: 'balances-snap-1',
        server_ts_ms: 1_700_000_100_000,
        payload: {
          balances: {
            count: 1,
            latest_ts_ms: 1_700_000_100_000,
          },
        },
      });
    });

    await expect(page.getByText('USDC', { exact: true })).toBeVisible();
    await expect(page.getByText('Net Equity (Σ MV): $2000.00')).toBeVisible();
    await expect(page.getByText('PLUME', { exact: true })).toHaveCount(0);
    expect(balanceRequests).toHaveLength(requestsAtHealthyIdleStart + 1);
  });
});

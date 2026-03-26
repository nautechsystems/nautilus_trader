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

const REALTIME_FLAGS = [
  'fluxboard:feature:realtime-standard',
  'fluxboard:feature:realtime-standard-trades',
] as const;

const makeTradeRow = (overrides: Partial<TradeRowFixture> = {}): TradeRowFixture => ({
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
});

async function installTradesTestRuntime(page: Parameters<typeof test>[0]['page']) {
  await page.addInitScript(({ flags }) => {
    window.localStorage.clear();
    for (const flag of flags) {
      window.localStorage.setItem(flag, 'true');
    }

    const listeners = new Map<string, Set<(payload?: any) => void>>();
    const socketEmits: Array<{ event: string; payload: any }> = [];
    const listenerOps: Array<{ op: 'on' | 'off'; event: string }> = [];

    const getBucket = (event: string) => {
      let bucket = listeners.get(event);
      if (!bucket) {
        bucket = new Set();
        listeners.set(event, bucket);
      }
      return bucket;
    };

    const emitToListeners = (event: string, payload?: any) => {
      for (const handler of listeners.get(event) ?? []) {
        handler(payload);
      }
    };

    const testSocket: any = {
      connected: true,
      id: 'pw-trades-standard-socket',
      io: {
        reconnect: () => {},
        engine: {
          transport: {
            close: () => {},
          },
        },
      },
      on(event: string, handler: (payload?: any) => void) {
        listenerOps.push({ op: 'on', event });
        getBucket(event).add(handler);
        return testSocket;
      },
      off(event: string, handler?: (payload?: any) => void) {
        listenerOps.push({ op: 'off', event });
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
              // The standard steady-state path is Socket.IO; capability metadata still
              // advertises polling-only recovery because replay is not available yet.
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
        emitToListeners(event, payload);
        return true;
      },
      connect() {
        if (!testSocket.connected) {
          testSocket.connected = true;
          emitToListeners('connect');
        }
        return testSocket;
      },
      disconnect() {
        if (testSocket.connected) {
          testSocket.connected = false;
          emitToListeners('disconnect', 'io client disconnect');
        }
        return testSocket;
      },
      removeAllListeners() {
        listeners.clear();
        return testSocket;
      },
      __emitServer(event: string, payload?: any) {
        emitToListeners(event, payload);
      },
    };

    (window as any).__fluxboardTestSocket = testSocket;
    (window as any).__fluxboardTestSocketFactory = () => testSocket;
    (window as any).__fluxboardSocketEmits = socketEmits;
    (window as any).__fluxboardSocketListenerOps = listenerOps;
  }, { flags: [...REALTIME_FLAGS] });
}

test.describe('Trades realtime cutover', () => {
  test('uses standard subscribe, ignores legacy steady-state events, and stays fail-closed after reconnect', async ({ page, baseURL }) => {
    const tradesRequests: string[] = [];
    const deltaRequests: string[] = [];

    await installTradesTestRuntime(page);

    await page.route('**/api/v1/trades?**', async (route) => {
      tradesRequests.push(route.request().url());
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
            limit: 50,
            offset: 0,
            page: 1,
            page_size: 50,
            last_seq: 2,
            stream_id: 'trades-main',
            snapshot_revision: 'snap-1',
            has_more: false,
            next_offset: null,
            next_cursor: null,
            sort: 'ts_desc',
            realtime: {
              contract_version: 2,
              surface: 'trades',
              profile: 'default',
              surface_query_key: 'trades|profile=default',
              stream_id: 'trades-main',
              snapshot_revision: 'snap-1',
              last_seq: 2,
              capabilities: {
                // Recovery remains invalidate-only/polling-only even though steady-state
                // updates arrive on realtime_event.
                recovery_mode: 'invalidate_only',
                replay_supported: false,
                transport_mode: 'polling_only',
              },
            },
          },
        }),
      });
    });

    await page.route('**/api/v1/trades/delta?**', async (route) => {
      deltaRequests.push(route.request().url());
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          ok: true,
          data: {
            rows: [],
            last_seq: 2,
            reset_required: false,
            stream_id: 'trades-main',
            snapshot_revision: 'snap-1',
          },
        }),
      });
    });

    await page.goto(`${baseURL ?? ''}/trades`);

    await expect(page.getByRole('button', { name: 'Export CSV' })).toBeVisible();
    await expect(page.getByText('OLD')).toBeVisible();
    await expect(page.getByText('NEW')).toBeVisible();
    await expect.poll(() =>
      page.evaluate(() =>
        ((window as any).__fluxboardSocketListenerOps as Array<{ op: string; event: string }>)
          .filter((entry) => entry.op === 'on' && entry.event === 'trade_update')
          .length
      )
    ).toBe(0);

    const requestUrl = new URL(tradesRequests[0]);
    expect(requestUrl.searchParams.get('contract_version')).toBe('2');

    const subscribePayload = await page.evaluate(() =>
      ((window as any).__fluxboardSocketEmits as Array<{ event: string; payload: any }>)
        .find((entry) => entry.event === 'subscribe')?.payload ?? null
    );
    expect(subscribePayload).toMatchObject({
      contract_version: 2,
      surface: 'trades',
      stream_id: 'trades-main',
      snapshot_revision: 'snap-1',
      resume_from_seq: 2,
    });

    await page.waitForTimeout(1_200);
    expect(deltaRequests).toHaveLength(0);

    await page.evaluate((trade) => {
      (window as any).__fluxboardTestSocket.__emitServer('trade_update', trade);
    }, makeTradeRow({
      row_id: 'legacy-trade',
      seq: 99,
      ts: 99,
      time: '2025-01-01T00:01:39.000Z',
      coin: 'LEGACY',
      trade_id: 'legacy-trade-99',
      exch_id: 'legacy-exec-99',
      order_id: 'legacy-order-99',
      signal_id: 'legacy-signal-99',
    }));
    await page.waitForTimeout(100);
    await expect(page.getByText('LEGACY')).toHaveCount(0);
    expect(tradesRequests).toHaveLength(1);
    expect(deltaRequests).toHaveLength(0);

    await page.evaluate((trade) => {
      (window as any).__fluxboardTestSocket.__emitServer('realtime_event', {
        contract_version: 2,
        surface: 'trades',
        profile: 'default',
        stream_id: 'trades-main',
        snapshot_revision: 'snap-1',
        kind: 'delta_batch',
        seq: 3,
        server_ts_ms: 1_700_000_003_000,
        payload: {
          trades: [trade],
        },
      });
    }, makeTradeRow({
      row_id: 'live-standard',
      version: 1,
      seq: 12,
      ts: 12,
      time: '2025-01-01T00:00:12.000Z',
      coin: 'OMEGA',
      exchange: 'bybit',
      side: 'buy',
      price: 112,
      qty: 2,
      mv: 224,
      fee: 0.2,
      trade_id: 'trade-12',
      exch_id: 'exec-12',
      order_id: 'order-12',
      signal_id: 'signal-12',
    }));

    await expect(page.getByText('OMEGA')).toBeVisible();
    await expect(page.getByText(/^LIVE$/)).toBeVisible();

    await page.evaluate(() => {
      (window as any).__fluxboardTestSocket.__emitServer('realtime_event', {
        contract_version: 2,
        surface: 'trades',
        profile: 'default',
        stream_id: 'trades-main',
        snapshot_revision: 'snap-1',
        kind: 'recovery_required',
        seq: 4,
        server_ts_ms: 1_700_000_004_000,
        reason: 'capability_withdrawn',
        payload: {},
      });
    });

    await expect(page.getByText('MANUAL REFRESH REQUIRED')).toBeVisible();

    await page.evaluate(() => {
      const socket = (window as any).__fluxboardTestSocket;
      socket.disconnect();
      socket.connect();
    });
    await page.waitForTimeout(250);
    await expect(page.getByText('MANUAL REFRESH REQUIRED')).toBeVisible();
    expect(tradesRequests).toHaveLength(1);
    expect(deltaRequests).toHaveLength(0);

    await expect.poll(() =>
      page.evaluate(() =>
        ((window as any).__fluxboardSocketEmits as Array<{ event: string; payload: any }>)
          .filter((entry) => entry.event === 'unsubscribe' && entry.payload?.surface === 'trades')
          .length
      )
    ).toBe(1);
  });
});

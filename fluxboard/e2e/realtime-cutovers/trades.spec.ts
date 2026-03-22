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

test.describe('Trades realtime cutover', () => {
  test('replays only while recovering and uses the standard gap cursor', async ({ page, baseURL }) => {
    const deltaUrls: string[] = [];

    await page.addInitScript(() => {
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
        id: 'pw-trades-socket',
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
    });

    await page.route('**/api/v1/trades?**', async (route) => {
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
      deltaUrls.push(route.request().url());
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          ok: true,
          data: {
            rows: [
              makeTradeRow({
                row_id: 'gap-3',
                seq: 3,
                ts: 3,
                time: '2025-01-01T00:00:03.000Z',
                coin: 'GAP3',
                side: 'sell',
                price: 103,
              }),
              makeTradeRow({
                row_id: 'gap-4',
                seq: 4,
                ts: 4,
                time: '2025-01-01T00:00:04.000Z',
                coin: 'GAP4',
                price: 104,
              }),
              makeTradeRow({
                row_id: 'gap-5',
                seq: 5,
                ts: 5,
                time: '2025-01-01T00:00:05.000Z',
                coin: 'GAP5',
                price: 105,
              }),
            ],
            last_seq: 5,
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

    await page.waitForTimeout(1_200);
    expect(deltaUrls).toHaveLength(0);

    await page.evaluate(() => {
      (window as any).__fluxboardTestSocket.__emitServer('trade_update', {
        op: 'upsert',
        row_id: 'gap-5',
        version: 1,
        seq: 5,
        ts: 5,
        time: '2025-01-01T00:00:05.000Z',
        coin: 'GAP5',
        exchange: 'bybit',
        side: 'buy',
        price: 105,
        qty: 1,
        mv: 105,
        fee: 0.1,
        trade_id: 'trade-gap-5',
        exch_id: 'exec-gap-5',
        order_id: 'order-gap-5',
        signal_id: 'signal-gap-5',
        stream_id: 'trades-main',
        snapshot_revision: 'snap-1',
      });
    });

    await expect(page.getByText('RECOVERING - Replaying…')).toBeVisible();
    await expect.poll(() => deltaUrls.length, { timeout: 4_000 }).toBe(1);

    const deltaQuery = new URL(deltaUrls[0]);
    expect(deltaQuery.searchParams.get('since_seq')).toBe('2');
    expect(deltaQuery.searchParams.get('stream_id')).toBe('trades-main');
    expect(deltaQuery.searchParams.get('snapshot_revision')).toBe('snap-1');
    expect(deltaQuery.searchParams.get('after')).toBeNull();

    await expect(page.getByText('GAP5')).toBeVisible();
    await expect(page.getByText(/^LIVE$/)).toBeVisible();
  });
});

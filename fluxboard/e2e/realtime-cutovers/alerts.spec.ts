import { expect, test } from '@playwright/test';

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

const makeAlert = (overrides: Partial<AlertFixture> = {}): AlertFixture => ({
  id: 'alert-1',
  level: 'WARNING',
  severity: 'WARNING',
  title: 'Spread drift',
  message: 'Spread drift widened',
  timestamp: 1_700_000_000,
  ts: 1_700_000_000,
  details: {},
  ...overrides,
});

test.describe('Alerts realtime cutover', () => {
  test('uses standard subscribe and summary invalidation recovery when the standard surface is enabled', async ({ page, baseURL }) => {
    const alertRequests: string[] = [];

    await page.addInitScript(() => {
      window.localStorage.setItem('fluxboard:feature:realtime-standard', 'true');
      window.localStorage.setItem('fluxboard:feature:realtime-standard-alerts', 'true');

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
        id: 'pw-alerts-socket',
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
    });

    await page.route('**/api/v1/alerts*', async (route) => {
      alertRequests.push(route.request().url());
      const requestNumber = alertRequests.length;
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

      if (requestNumber === 2) {
        await new Promise((resolve) => setTimeout(resolve, 250));
      }

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
            realtime: {
              contract_version: 2,
              surface: 'alerts',
              profile: 'default',
              surface_query_key: 'alerts|profile=default',
              stream_id: 'alerts-main',
              snapshot_revision: 'alerts-snap-1',
              last_seq: requestNumber === 1 ? 0 : 1,
              capabilities: {
                recovery_mode: 'invalidate_only',
                replay_supported: false,
                transport_mode: 'polling_only',
              },
            },
          },
        }),
      });
    });

    await page.goto(`${baseURL ?? ''}/alerts`);

    await expect(page.getByRole('cell', { name: 'Initial warning' }).first()).toBeVisible();
    await expect(page.getByText(/^LIVE$/)).toBeVisible();
    const requestsAtHealthyIdleStart = alertRequests.length;
    await page.waitForTimeout(1_200);
    const requestsBeforeInvalidate = alertRequests.length;
    expect(requestsAtHealthyIdleStart).toBeGreaterThanOrEqual(1);
    expect(requestsBeforeInvalidate).toBe(requestsAtHealthyIdleStart);

    await page.evaluate(() => {
      (window as any).__fluxboardTestSocket.__emitServer('realtime_event', {
        contract_version: 2,
        surface: 'alerts',
        profile: 'default',
        stream_id: 'alerts-main',
        kind: 'invalidate',
        seq: 1,
        snapshot_revision: 'alerts-snap-1',
        server_ts_ms: 1_700_000_100_000,
        payload: {
          alerts: {
            count: 1,
            latest_ts_ms: 1_700_000_100_000,
          },
        },
      });
    });

    await expect(page.getByText('RECOVERING')).toBeVisible();
    await expect(page.getByRole('cell', { name: 'Recovered alert after summary refresh' }).first()).toBeVisible();
    await expect(page.getByText(/^LIVE$/)).toBeVisible();
    expect(alertRequests.length).toBeGreaterThan(requestsBeforeInvalidate);
  });
});

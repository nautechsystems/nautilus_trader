import { expect, test } from '@playwright/test';

type SignalStrategyFixture = {
  id: string;
  params: {
    bot_on: string;
    cex_bid_edge: string;
    cex_ask_edge: string;
    pool_edge: string;
    qty: string;
    slippage_bps: string;
  };
  legs: {
    A: {
      exchange: string;
      coin: string;
      decision_bid: number;
      decision_ask: number;
      net_edge_bps: number;
      update_time: string;
    };
    B: {
      exchange: string;
      coin: string;
      decision_bid: number;
      decision_ask: number;
      net_edge_bps: number;
      update_time: string;
    };
  };
  balances_ok: boolean;
  edge2_bps: number;
  risk_delta: number;
};

const REALTIME_FLAGS = [
  'fluxboard:feature:realtime-standard',
  'fluxboard:feature:realtime-standard-signal',
] as const;

const makeSignalStrategy = (
  id: string,
  overrides: Partial<SignalStrategyFixture> = {},
): SignalStrategyFixture => ({
  id,
  params: {
    bot_on: '1',
    cex_bid_edge: '10',
    cex_ask_edge: '10',
    pool_edge: '10',
    qty: '100',
    slippage_bps: '50',
  },
  legs: {
    A: {
      exchange: 'bybit',
      coin: 'PLUME',
      decision_bid: 1.0,
      decision_ask: 1.01,
      net_edge_bps: 10,
      update_time: '2026-03-23 00:00:00',
    },
    B: {
      exchange: 'rooster',
      coin: 'WPLUME',
      decision_bid: 1.02,
      decision_ask: 1.03,
      net_edge_bps: 12,
      update_time: '2026-03-23 00:00:00',
    },
  },
  balances_ok: true,
  edge2_bps: 5,
  risk_delta: 25,
  ...overrides,
});

async function installSignalTestRuntime(page: Parameters<typeof test>[0]['page']) {
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
      id: 'pw-signal-standard-socket',
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

test.describe('Signal realtime cutover', () => {
  test('uses standard subscribe, ignores legacy steady-state events, and recovers after reconnect/invalidate', async ({ page, baseURL }) => {
    let signalRequests = 0;

    await installSignalTestRuntime(page);

    await page.route('**/api/v1/signals?**', async (route) => {
      signalRequests += 1;
      const payload = signalRequests === 1
        ? {
            ok: true,
            data: {
              strategies: [
                makeSignalStrategy('signal-001'),
              ],
              server_time: '2026-03-23 00:00:00',
              server_ts_ms: 1_700_000_000_000,
              realtime: {
                contract_version: 2,
                surface: 'signal',
                profile: 'default',
                surface_query_key: 'signal|profile=default',
                stream_id: 'signals-main',
                snapshot_revision: 'sig-snap-1',
                last_seq: 0,
                capabilities: {
                  recovery_mode: 'invalidate_only',
                  replay_supported: false,
                  transport_mode: 'polling_only',
                },
              },
            },
          }
        : {
            ok: true,
            data: {
              strategies: [
                makeSignalStrategy('signal-001'),
                makeSignalStrategy('signal-002', {
                  edge2_bps: 15,
                  risk_delta: 40,
                }),
              ],
              server_time: '2026-03-23 00:00:05',
              server_ts_ms: 1_700_000_005_000,
              realtime: {
                contract_version: 2,
                surface: 'signal',
                profile: 'default',
                surface_query_key: 'signal|profile=default',
                stream_id: 'signals-main',
                snapshot_revision: 'sig-snap-1',
                last_seq: 2,
                capabilities: {
                  recovery_mode: 'invalidate_only',
                  replay_supported: false,
                  transport_mode: 'polling_only',
                },
              },
            },
          };

      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(payload),
      });
    });

    await page.goto(`${baseURL ?? ''}/signal`);

    await expect(page.getByText('signal-001')).toBeVisible();
    await expect.poll(() => signalRequests).toBe(1);
    await expect.poll(() =>
      page.evaluate(() =>
        ((window as any).__fluxboardSocketListenerOps as Array<{ op: string; event: string }>)
          .filter((entry) => entry.op === 'on' && ['market_update', 'signal_delta'].includes(entry.event))
          .length
      )
    ).toBe(0);

    const subscribePayload = await page.evaluate(() =>
      ((window as any).__fluxboardSocketEmits as Array<{ event: string; payload: any }>)
        .find((entry) => entry.event === 'subscribe')?.payload ?? null
    );
    expect(subscribePayload).toMatchObject({
      contract_version: 2,
      surface: 'signal',
      stream_id: 'signals-main',
      snapshot_revision: 'sig-snap-1',
      resume_from_seq: 0,
    });

    await page.evaluate((payload) => {
      (window as any).__fluxboardTestSocket.__emitServer('market_update', {
        strategies: [payload],
        server_time: '2026-03-23 00:00:01',
        server_ts_ms: 1_700_000_001_000,
      });
    }, makeSignalStrategy('legacy-signal', {
      edge2_bps: 77,
      risk_delta: 77,
    }));
    await page.waitForTimeout(100);
    await expect(page.getByText('legacy-signal')).toHaveCount(0);
    expect(signalRequests).toBe(1);

    await page.evaluate((payload) => {
      (window as any).__fluxboardTestSocket.__emitServer('realtime_event', {
        contract_version: 2,
        surface: 'signal',
        profile: 'default',
        stream_id: 'signals-main',
        snapshot_revision: 'sig-snap-1',
        kind: 'delta_batch',
        seq: 1,
        server_ts_ms: 1_700_000_001_000,
        payload: {
          signals: [payload],
          strategies: {
            changed: [payload.id],
          },
        },
      });
    }, {
      id: 'signal-001',
      legs: {
        A: {
          coin: 'ORBIT',
        },
      },
      risk_delta: 55,
    });

    await expect(page.getByText('55.0000')).toBeVisible();

    await page.evaluate(() => {
      const socket = (window as any).__fluxboardTestSocket;
      socket.disconnect();
      socket.connect();
    });

    await expect.poll(() => signalRequests, { timeout: 5_000 }).toBe(2);
    await expect(page.getByText('signal-002')).toBeVisible();

    await page.evaluate(() => {
      (window as any).__fluxboardTestSocket.__emitServer('realtime_event', {
        contract_version: 2,
        surface: 'signal',
        profile: 'default',
        stream_id: 'signals-main',
        snapshot_revision: 'sig-snap-1',
        kind: 'invalidate',
        seq: 2,
        server_ts_ms: 1_700_000_002_000,
        reason: 'changed_ids_missing_payload',
        payload: {},
      });
    });

    await expect.poll(() => signalRequests, { timeout: 5_000 }).toBe(3);
    await expect(page.getByText('signal-002')).toBeVisible();

    await page.getByRole('link', { name: 'Params' }).click();

    await expect.poll(() =>
      page.evaluate(() =>
        ((window as any).__fluxboardSocketEmits as Array<{ event: string; payload: any }>)
          .filter((entry) => entry.event === 'unsubscribe' && entry.payload?.surface === 'signal')
          .length >= 1
      )
    ).toBe(true);
  });
});

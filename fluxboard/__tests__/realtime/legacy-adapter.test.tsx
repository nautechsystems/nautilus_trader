import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { disconnectSocket } from '@/sockets';
import * as useWebSocketModule from '@/hooks/useWebSocket';
import { REALTIME_STANDARD_STORAGE_FLAGS } from '@/lib/realtime/constants';

const { useWebSocket } = useWebSocketModule;
const registerSharedWebSocketBridge = (useWebSocketModule as any).registerSharedWebSocketBridge as
  | ((bridge: unknown) => void)
  | undefined;
const resetSharedWebSocketBridgeForTests = (useWebSocketModule as any).resetSharedWebSocketBridgeForTests as
  | (() => void)
  | undefined;

type EventHandler = (...args: any[]) => void;

type FakeSocketControl = {
  socket: any;
  emit: (event: string, ...args: any[]) => void;
};

type RealtimeFlagName = keyof typeof REALTIME_STANDARD_STORAGE_FLAGS;
type DynamicHookRuntime = Awaited<ReturnType<typeof loadFlagAwareHookRuntime>>;

function createFakeSocket(): FakeSocketControl {
  const listeners = new Map<string, Set<EventHandler>>();

  const emit = (event: string, ...args: any[]) => {
    const handlers = listeners.get(event);
    if (!handlers) {
      return;
    }
    for (const handler of handlers) {
      handler(...args);
    }
  };

  const socket: any = {
    connected: false,
    io: {
      reconnect: vi.fn(),
      engine: {
        transport: {
          close: vi.fn(),
        },
      },
    },
    on: vi.fn((event: string, handler: EventHandler) => {
      const handlers = listeners.get(event) ?? new Set<EventHandler>();
      handlers.add(handler);
      listeners.set(event, handlers);
      return socket;
    }),
    off: vi.fn((event: string, handler?: EventHandler) => {
      if (!handler) {
        listeners.delete(event);
        return socket;
      }
      const handlers = listeners.get(event);
      handlers?.delete(handler);
      if (handlers && handlers.size === 0) {
        listeners.delete(event);
      }
      return socket;
    }),
    emit: vi.fn((event: string, ...args: any[]) => {
      emit(event, ...args);
      return socket;
    }),
    connect: vi.fn(() => {
      socket.connected = true;
      emit('connect');
      return socket;
    }),
    disconnect: vi.fn(() => {
      socket.connected = false;
      emit('disconnect', 'io client disconnect');
      return socket;
    }),
    removeAllListeners: vi.fn(() => {
      listeners.clear();
      return socket;
    }),
  };

  return { socket, emit };
}

function setRealtimeFlags(flags: Partial<Record<RealtimeFlagName, boolean>>): void {
  localStorage.clear();
  for (const [name, enabled] of Object.entries(flags) as Array<[RealtimeFlagName, boolean | undefined]>) {
    if (!enabled) {
      continue;
    }
    localStorage.setItem(REALTIME_STANDARD_STORAGE_FLAGS[name], '1');
  }
}

async function loadFlagAwareHookRuntime(
  flags: Partial<Record<RealtimeFlagName, boolean>>,
): Promise<{
  hookRuntime: typeof import('@/hooks/useWebSocket');
  socketsRuntime: typeof import('@/sockets');
}> {
  setRealtimeFlags(flags);
  vi.resetModules();
  const [hookRuntime, socketsRuntime] = await Promise.all([
    import('@/hooks/useWebSocket'),
    import('@/sockets'),
  ]);
  return { hookRuntime, socketsRuntime };
}

describe('useWebSocket legacy adapter foundation', () => {
  let socketControl: FakeSocketControl;
  let resetDynamicRuntime: (() => void) | null;

  beforeEach(() => {
    localStorage.clear();
    socketControl = createFakeSocket();
    (window as any).__fluxboardTestSocketFactory = () => socketControl.socket;
    resetDynamicRuntime = null;
  });

  afterEach(() => {
    resetDynamicRuntime?.();
    resetDynamicRuntime = null;
    disconnectSocket();
    delete (window as any).__fluxboardTestSocketFactory;
    resetSharedWebSocketBridgeForTests?.();
    localStorage.clear();
    vi.restoreAllMocks();
  });

  it('keeps the two-argument legacy subscription path and raw payload shape intact', () => {
    const handler = vi.fn();

    const { unmount } = renderHook(() => useWebSocket('legacy:trades', handler));

    const subscription = socketControl.socket.on.mock.calls.find(
      ([event]: [string]) => event === 'legacy:trades',
    );

    expect(subscription).toBeDefined();

    const legacyPayload = {
      rows: [{ row_id: 'trade-1', qty: '1.0' }],
      next_cursor: 'cursor-1',
    };

    act(() => {
      socketControl.emit('legacy:trades', legacyPayload);
    });

    expect(handler).toHaveBeenCalledWith(legacyPayload);

    unmount();

    expect(socketControl.socket.off).toHaveBeenCalledWith('legacy:trades', subscription?.[1]);
  });

  it('stays on the injected legacy subscription path when bridge mode resolves to legacy', () => {
    const handler = vi.fn();
    const legacyUnsubscribe = vi.fn();
    const legacyListeners: Array<(payload: unknown) => void> = [];
    const legacySubscribe = vi.fn((event: string, legacyHandler: (payload: unknown) => void) => {
      legacyListeners.push(legacyHandler);
      return legacyUnsubscribe;
    });
    const bridgeSubscribe = vi.fn(() => vi.fn());
    const resolveMode = vi.fn(() => 'legacy' as const);

    const { unmount } = renderHook(() =>
      useWebSocket('legacy:signal', handler, {
        surface: 'signal',
        subscribe: legacySubscribe,
        bridge: {
          resolveMode,
          subscribe: bridgeSubscribe,
        },
      }),
    );

    expect(resolveMode).toHaveBeenCalledWith({
      event: 'legacy:signal',
      surface: 'signal',
    });
    expect(legacySubscribe).toHaveBeenCalledTimes(1);
    expect(legacySubscribe).toHaveBeenCalledWith('legacy:signal', expect.any(Function));
    expect(bridgeSubscribe).not.toHaveBeenCalled();

    const rawPayload = { strategy_id: 'signal-1', legacy_only: true };

    act(() => {
      legacyListeners[0]?.(rawPayload);
    });

    expect(handler).toHaveBeenCalledWith(rawPayload);

    unmount();

    expect(legacyUnsubscribe).toHaveBeenCalledTimes(1);
  });

  it('routes explicit bridge overrides through the injected bridge subscription', () => {
    const handler = vi.fn();
    const legacySubscribe = vi.fn(() => vi.fn());
    const resolveMode = vi.fn(() => 'standard' as const);
    const bridgeUnsubscribe = vi.fn();
    let bridgeHandler: ((payload: unknown) => void) | undefined;
    const bridgeSubscribe = vi.fn((options: {
      event: string;
      surface?: string;
      legacySubscribe: typeof legacySubscribe;
      handler: (payload: unknown) => void;
    }) => {
      bridgeHandler = options.handler;
      return bridgeUnsubscribe;
    });

    const { unmount } = renderHook(() =>
      useWebSocket('standard:alerts', handler, {
        surface: 'alerts',
        subscribe: legacySubscribe,
        bridge: {
          resolveMode,
          subscribe: bridgeSubscribe,
        },
      }),
    );

    expect(resolveMode).toHaveBeenCalledWith({
      event: 'standard:alerts',
      surface: 'alerts',
    });
    expect(bridgeSubscribe).toHaveBeenCalledTimes(1);
    expect(bridgeSubscribe).toHaveBeenCalledWith({
      event: 'standard:alerts',
      surface: 'alerts',
      legacySubscribe,
      handler: expect.any(Function),
    });
    expect(legacySubscribe).not.toHaveBeenCalled();

    const bridgedPayload = { id: 'alert-1', level: 'warning', source: 'standard' };

    act(() => {
      bridgeHandler?.(bridgedPayload);
    });

    expect(handler).toHaveBeenCalledWith(bridgedPayload);

    unmount();

    expect(bridgeUnsubscribe).toHaveBeenCalledTimes(1);
  });

  it('keeps a surface on the legacy path when realtime standard flags are off', async () => {
    const { hookRuntime, socketsRuntime }: DynamicHookRuntime = await loadFlagAwareHookRuntime({});
    resetDynamicRuntime = () => {
      hookRuntime.resetSharedWebSocketBridgeForTests?.();
      socketsRuntime.disconnectSocket();
    };
    const handler = vi.fn();
    const legacyUnsubscribe = vi.fn();
    const legacyListeners: Array<(payload: unknown) => void> = [];
    const legacySubscribe = vi.fn((event: string, legacyHandler: (payload: unknown) => void) => {
      legacyListeners.push(legacyHandler);
      return legacyUnsubscribe;
    });
    const sharedBridgeSubscribe = vi.fn(() => vi.fn());

    hookRuntime.registerSharedWebSocketBridge({
      subscribe: sharedBridgeSubscribe,
    });

    const { unmount } = renderHook(() =>
      hookRuntime.useWebSocket('signal:update', handler, {
        surface: 'signal',
        subscribe: legacySubscribe,
      }),
    );

    expect(sharedBridgeSubscribe).not.toHaveBeenCalled();
    expect(legacySubscribe).toHaveBeenCalledTimes(1);
    expect(legacySubscribe).toHaveBeenCalledWith('signal:update', expect.any(Function));

    act(() => {
      legacyListeners[0]?.({ strategy_id: 'legacy-signal', source: 'legacy' });
    });

    expect(handler).toHaveBeenCalledWith({ strategy_id: 'legacy-signal', source: 'legacy' });

    unmount();

    expect(legacyUnsubscribe).toHaveBeenCalledTimes(1);
  });

  it('uses the registered shared bridge path when the realtime standard surface flags are on', async () => {
    const { hookRuntime, socketsRuntime }: DynamicHookRuntime = await loadFlagAwareHookRuntime({
      global: true,
      signal: true,
    });
    resetDynamicRuntime = () => {
      hookRuntime.resetSharedWebSocketBridgeForTests?.();
      socketsRuntime.disconnectSocket();
    };
    const handler = vi.fn();
    const legacySubscribe = vi.fn(() => vi.fn());
    const bridgeUnsubscribe = vi.fn();
    let bridgeHandler: ((payload: unknown) => void) | undefined;
    const sharedBridgeSubscribe = vi.fn((options: {
      event: string;
      surface?: string;
      legacySubscribe: typeof legacySubscribe;
      handler: (payload: unknown) => void;
    }) => {
      bridgeHandler = options.handler;
      return bridgeUnsubscribe;
    });

    hookRuntime.registerSharedWebSocketBridge({
      subscribe: sharedBridgeSubscribe,
    });

    const { unmount } = renderHook(() =>
      hookRuntime.useWebSocket('signal:update', handler, {
        surface: 'signal',
        subscribe: legacySubscribe,
      }),
    );

    expect(sharedBridgeSubscribe).toHaveBeenCalledTimes(1);
    expect(sharedBridgeSubscribe).toHaveBeenCalledWith({
      event: 'signal:update',
      surface: 'signal',
      legacySubscribe,
      handler: expect.any(Function),
    });
    expect(legacySubscribe).not.toHaveBeenCalled();

    act(() => {
      bridgeHandler?.({ strategy_id: 'shared-bridge', source: 'standard' });
    });

    expect(handler).toHaveBeenCalledWith({ strategy_id: 'shared-bridge', source: 'standard' });

    unmount();

    expect(bridgeUnsubscribe).toHaveBeenCalledTimes(1);
  });

  it('reacts to shared bridge registration after mount without waiting for an unrelated rerender', async () => {
    const { hookRuntime, socketsRuntime }: DynamicHookRuntime = await loadFlagAwareHookRuntime({
      global: true,
      signal: true,
    });
    resetDynamicRuntime = () => {
      hookRuntime.resetSharedWebSocketBridgeForTests?.();
      socketsRuntime.disconnectSocket();
    };
    const handler = vi.fn();
    const legacyUnsubscribe = vi.fn();
    const legacyListeners: Array<(payload: unknown) => void> = [];
    const legacySubscribe = vi.fn((event: string, legacyHandler: (payload: unknown) => void) => {
      legacyListeners.push(legacyHandler);
      return legacyUnsubscribe;
    });
    const bridgeUnsubscribe = vi.fn();
    let bridgeHandler: ((payload: unknown) => void) | undefined;
    const sharedBridgeSubscribe = vi.fn((options: {
      event: string;
      surface?: string;
      legacySubscribe: typeof legacySubscribe;
      handler: (payload: unknown) => void;
    }) => {
      bridgeHandler = options.handler;
      return bridgeUnsubscribe;
    });

    const { unmount } = renderHook(() =>
      hookRuntime.useWebSocket('signal:update', handler, {
        surface: 'signal',
        subscribe: legacySubscribe,
      }),
    );

    expect(legacySubscribe).toHaveBeenCalledTimes(1);
    expect(sharedBridgeSubscribe).not.toHaveBeenCalled();

    act(() => {
      hookRuntime.registerSharedWebSocketBridge({
        subscribe: sharedBridgeSubscribe,
      });
    });

    expect(legacyUnsubscribe).toHaveBeenCalledTimes(1);
    expect(sharedBridgeSubscribe).toHaveBeenCalledTimes(1);
    expect(sharedBridgeSubscribe).toHaveBeenCalledWith({
      event: 'signal:update',
      surface: 'signal',
      legacySubscribe,
      handler: expect.any(Function),
    });

    act(() => {
      bridgeHandler?.({ strategy_id: 'late-shared-bridge', source: 'standard' });
    });

    expect(handler).toHaveBeenCalledWith({ strategy_id: 'late-shared-bridge', source: 'standard' });

    unmount();

    expect(bridgeUnsubscribe).toHaveBeenCalledTimes(1);
  });

  it('does not churn legacy subscriptions without a surface when a shared bridge registers later', async () => {
    const { hookRuntime, socketsRuntime }: DynamicHookRuntime = await loadFlagAwareHookRuntime({
      global: true,
      signal: true,
    });
    resetDynamicRuntime = () => {
      hookRuntime.resetSharedWebSocketBridgeForTests?.();
      socketsRuntime.disconnectSocket();
    };
    const handler = vi.fn();
    const legacyUnsubscribe = vi.fn();
    const legacySubscribe = vi.fn(() => legacyUnsubscribe);

    const { unmount } = renderHook(() =>
      hookRuntime.useWebSocket('legacy:alerts', handler, {
        subscribe: legacySubscribe,
      }),
    );

    expect(legacySubscribe).toHaveBeenCalledTimes(1);

    act(() => {
      hookRuntime.registerSharedWebSocketBridge({
        subscribe: vi.fn(() => vi.fn()),
      });
    });

    expect(legacyUnsubscribe).not.toHaveBeenCalled();
    expect(legacySubscribe).toHaveBeenCalledTimes(1);

    unmount();

    expect(legacyUnsubscribe).toHaveBeenCalledTimes(1);
  });

  it('does not churn flag-off surfaced legacy subscriptions when a shared bridge registers later', async () => {
    const { hookRuntime, socketsRuntime }: DynamicHookRuntime = await loadFlagAwareHookRuntime({});
    resetDynamicRuntime = () => {
      hookRuntime.resetSharedWebSocketBridgeForTests?.();
      socketsRuntime.disconnectSocket();
    };
    const handler = vi.fn();
    const legacyUnsubscribe = vi.fn();
    const legacySubscribe = vi.fn(() => legacyUnsubscribe);

    const { unmount } = renderHook(() =>
      hookRuntime.useWebSocket('signal:update', handler, {
        surface: 'signal',
        subscribe: legacySubscribe,
      }),
    );

    expect(legacySubscribe).toHaveBeenCalledTimes(1);

    act(() => {
      hookRuntime.registerSharedWebSocketBridge({
        subscribe: vi.fn(() => vi.fn()),
      });
    });

    expect(legacyUnsubscribe).not.toHaveBeenCalled();
    expect(legacySubscribe).toHaveBeenCalledTimes(1);

    unmount();

    expect(legacyUnsubscribe).toHaveBeenCalledTimes(1);
  });

  it('honors resolveMode from the registered shared bridge before falling back to realtime flags', async () => {
    const { hookRuntime, socketsRuntime }: DynamicHookRuntime = await loadFlagAwareHookRuntime({
      global: true,
      signal: true,
    });
    resetDynamicRuntime = () => {
      hookRuntime.resetSharedWebSocketBridgeForTests?.();
      socketsRuntime.disconnectSocket();
    };
    const handler = vi.fn();
    const legacyUnsubscribe = vi.fn();
    const legacyListeners: Array<(payload: unknown) => void> = [];
    const legacySubscribe = vi.fn((event: string, legacyHandler: (payload: unknown) => void) => {
      legacyListeners.push(legacyHandler);
      return legacyUnsubscribe;
    });
    const sharedBridgeSubscribe = vi.fn(() => vi.fn());
    const sharedResolveMode = vi.fn(() => 'legacy' as const);

    hookRuntime.registerSharedWebSocketBridge({
      resolveMode: sharedResolveMode,
      subscribe: sharedBridgeSubscribe,
    });

    const { unmount } = renderHook(() =>
      hookRuntime.useWebSocket('signal:update', handler, {
        surface: 'signal',
        subscribe: legacySubscribe,
      }),
    );

    expect(sharedResolveMode).toHaveBeenCalledWith({
      event: 'signal:update',
      surface: 'signal',
    });
    expect(sharedBridgeSubscribe).not.toHaveBeenCalled();
    expect(legacySubscribe).toHaveBeenCalledTimes(1);

    act(() => {
      legacyListeners[0]?.({ strategy_id: 'shared-resolve-mode', source: 'legacy' });
    });

    expect(handler).toHaveBeenCalledWith({ strategy_id: 'shared-resolve-mode', source: 'legacy' });

    unmount();

    expect(legacyUnsubscribe).toHaveBeenCalledTimes(1);
  });

  it('prefers an explicit per-call bridge override over the registered shared bridge', async () => {
    const { hookRuntime, socketsRuntime }: DynamicHookRuntime = await loadFlagAwareHookRuntime({
      global: true,
      alerts: true,
    });
    resetDynamicRuntime = () => {
      hookRuntime.resetSharedWebSocketBridgeForTests?.();
      socketsRuntime.disconnectSocket();
    };
    const handler = vi.fn();
    const legacySubscribe = vi.fn(() => vi.fn());
    const sharedBridgeSubscribe = vi.fn(() => vi.fn());
    const overrideUnsubscribe = vi.fn();
    const overrideBridgeSubscribe = vi.fn(() => overrideUnsubscribe);

    hookRuntime.registerSharedWebSocketBridge({
      subscribe: sharedBridgeSubscribe,
    });

    const { unmount } = renderHook(() =>
      hookRuntime.useWebSocket('override:alerts', handler, {
        surface: 'alerts',
        subscribe: legacySubscribe,
        bridge: {
          subscribe: overrideBridgeSubscribe,
        },
      }),
    );

    expect(overrideBridgeSubscribe).toHaveBeenCalledTimes(1);
    expect(sharedBridgeSubscribe).not.toHaveBeenCalled();

    unmount();

    expect(overrideUnsubscribe).toHaveBeenCalledTimes(1);
  });

  it('does not duplicate bridge subscriptions when only the handler changes and cleans up on unmount', () => {
    const firstHandler = vi.fn();
    const secondHandler = vi.fn();
    const legacySubscribe = vi.fn(() => vi.fn());
    const bridgeUnsubscribe = vi.fn();
    let bridgeHandler: ((payload: unknown) => void) | undefined;
    const options = {
      surface: 'trades',
      subscribe: legacySubscribe,
      bridge: {
        resolveMode: () => 'standard' as const,
        subscribe: vi.fn((bridgeOptions: {
          handler: (payload: unknown) => void;
          legacySubscribe: typeof legacySubscribe;
        }) => {
          bridgeHandler = bridgeOptions.handler;
          return bridgeUnsubscribe;
        }),
      },
    };

    const { rerender, unmount } = renderHook(
      ({ activeHandler }) => useWebSocket('standard:trades', activeHandler, options),
      { initialProps: { activeHandler: firstHandler } },
    );

    expect(options.bridge.subscribe).toHaveBeenCalledTimes(1);

    rerender({ activeHandler: secondHandler });

    expect(options.bridge.subscribe).toHaveBeenCalledTimes(1);
    expect(bridgeUnsubscribe).not.toHaveBeenCalled();

    act(() => {
      bridgeHandler?.({ row_id: 'trade-2', source: 'bridge' });
    });

    expect(firstHandler).not.toHaveBeenCalled();
    expect(secondHandler).toHaveBeenCalledWith({ row_id: 'trade-2', source: 'bridge' });

    unmount();

    expect(bridgeUnsubscribe).toHaveBeenCalledTimes(1);
  });

  it('tears down the old subscription when the resolved mode flips between legacy and standard', () => {
    const handler = vi.fn();
    let mode: 'legacy' | 'standard' = 'legacy';
    const legacyUnsubscribe = vi.fn();
    const bridgeUnsubscribe = vi.fn();
    const legacySubscribe = vi.fn(() => legacyUnsubscribe);
    const bridgeSubscribe = vi.fn(() => bridgeUnsubscribe);
    const resolveMode = vi.fn(() => mode);
    const options = {
      surface: 'signal',
      subscribe: legacySubscribe,
      bridge: {
        resolveMode,
        subscribe: bridgeSubscribe,
      },
    };

    const { rerender, unmount } = renderHook(() =>
      useWebSocket('signal:update', handler, options),
    );

    expect(legacySubscribe).toHaveBeenCalledTimes(1);
    expect(bridgeSubscribe).not.toHaveBeenCalled();

    mode = 'standard';
    rerender();

    expect(legacyUnsubscribe).toHaveBeenCalledTimes(1);
    expect(bridgeSubscribe).toHaveBeenCalledTimes(1);

    mode = 'legacy';
    rerender();

    expect(bridgeUnsubscribe).toHaveBeenCalledTimes(1);
    expect(legacySubscribe).toHaveBeenCalledTimes(2);

    unmount();

    expect(legacyUnsubscribe).toHaveBeenCalledTimes(2);
  });
});

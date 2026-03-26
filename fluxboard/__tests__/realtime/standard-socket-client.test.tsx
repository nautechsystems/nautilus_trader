import { renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { useStandardWebSocketSubscription } from '@/hooks/useWebSocket';

type EventHandler = (...args: any[]) => void;

type FakeSocketControl = {
  socket: any;
  emit: (event: string, ...args: any[]) => void;
};

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

  return {
    socket,
    emit,
  };
}

const socketsMock = vi.hoisted(() => {
  const unsubscribe = vi.fn();
  return {
    socket: {
      on: vi.fn(),
      off: vi.fn(),
      connected: true,
    },
    standardSocketClient: {
      subscribe: vi.fn(() => unsubscribe),
    },
    unsubscribe,
  };
});

vi.mock('@/sockets', () => ({
  socket: socketsMock.socket,
  standardSocketClient: socketsMock.standardSocketClient,
}));

describe('useStandardWebSocketSubscription', () => {
  const lineage = {
    contract_version: 2,
    surface: 'signal',
    profile: 'tokenmm',
    surface_query_key: 'signal:tokenmm:canonical',
    stream_id: 'signal-main',
    snapshot_revision: 17,
    last_seq: 3,
  };

  beforeEach(() => {
    socketsMock.socket.on.mockReset();
    socketsMock.socket.off.mockReset();
    socketsMock.standardSocketClient.subscribe.mockReset();
    socketsMock.unsubscribe.mockReset();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it('subscribes through the shared standard socket client and cleans up on unmount', async () => {
    let currentResumeFromSeq = 3;
    const onEvent = vi.fn();
    const onSubscribed = vi.fn();

    const { unmount } = renderHook(() =>
      useStandardWebSocketSubscription({
        enabled: true,
        lineage,
        resumeFromSeq: () => currentResumeFromSeq,
        onEvent,
        onSubscribed,
      }),
    );

    await waitFor(() => {
      expect(socketsMock.standardSocketClient.subscribe).toHaveBeenCalledTimes(1);
    });

    const options = socketsMock.standardSocketClient.subscribe.mock.calls[0]?.[0];
    expect(options.lineage).toEqual(lineage);
    expect(options.resumeFromSeq()).toBe(3);

    currentResumeFromSeq = 8;
    expect(options.resumeFromSeq()).toBe(8);

    options.onSubscribed?.({ accepted: true, surface: 'signal' });
    expect(onSubscribed).toHaveBeenCalledWith(expect.objectContaining({ accepted: true, surface: 'signal' }));

    options.onEvent?.({ kind: 'delta_batch', surface: 'signal' });
    expect(onEvent).toHaveBeenCalledWith(expect.objectContaining({ kind: 'delta_batch', surface: 'signal' }));

    unmount();
    expect(socketsMock.unsubscribe).toHaveBeenCalledTimes(1);
  });

  it('surfaces subscription failures without silently downgrading to legacy transport', async () => {
    const onFailure = vi.fn();

    renderHook(() =>
      useStandardWebSocketSubscription({
        enabled: true,
        lineage,
        onEvent: vi.fn(),
        onFailure,
      }),
    );

    await waitFor(() => {
      expect(socketsMock.standardSocketClient.subscribe).toHaveBeenCalledTimes(1);
    });

    const options = socketsMock.standardSocketClient.subscribe.mock.calls[0]?.[0];
    options.onFailure?.({
      type: 'subscribe_rejected',
      reason: 'backend_kill_switch',
      requested: {
        contract_version: 2,
        surface: 'signal',
        profile: 'tokenmm',
        surface_query_key: 'signal:tokenmm:canonical',
        stream_id: 'signal-main',
        snapshot_revision: 17,
        resume_from_seq: 3,
      },
      ack: {
        accepted: false,
        reason: 'backend_kill_switch',
      },
    });

    expect(onFailure).toHaveBeenCalledWith(expect.objectContaining({
      type: 'subscribe_rejected',
      reason: 'backend_kill_switch',
    }));
  });

  it('does not churn subscriptions when rerendered with an equivalent lineage object', async () => {
    const onEvent = vi.fn();
    const { rerender, unmount } = renderHook(
      ({ currentLineage }) =>
        useStandardWebSocketSubscription({
          enabled: true,
          lineage: currentLineage,
          onEvent,
        }),
      {
        initialProps: {
          currentLineage: lineage,
        },
      },
    );

    await waitFor(() => {
      expect(socketsMock.standardSocketClient.subscribe).toHaveBeenCalledTimes(1);
    });

    rerender({
      currentLineage: {
        ...lineage,
      },
    });

    await waitFor(() => {
      expect(socketsMock.standardSocketClient.subscribe).toHaveBeenCalledTimes(1);
    });
    expect(socketsMock.unsubscribe).not.toHaveBeenCalled();

    unmount();
    expect(socketsMock.unsubscribe).toHaveBeenCalledTimes(1);
  });
});

describe('standard socket client runtime rebinding', () => {
  afterEach(() => {
    delete (window as any).__fluxboardTestSocketFactory;
    vi.resetModules();
  });

  it('re-subscribes active singleton subscriptions after the socket is destroyed and recreated', async () => {
    vi.doMock('@/stores', () => ({
      bumpGlobalResync: vi.fn(),
    }));

    const controls: FakeSocketControl[] = [];
    (window as any).__fluxboardTestSocketFactory = () => {
      const control = createFakeSocket();
      controls.push(control);
      return control.socket;
    };

    const socketsRuntime = await vi.importActual<typeof import('@/sockets')>('@/sockets');
    const onEvent = vi.fn();
    const onSubscribed = vi.fn();
    const lineage = {
      contract_version: 2,
      surface: 'signal',
      profile: 'default',
      surface_query_key: 'signal|profile=default',
      stream_id: 'signal-main',
      snapshot_revision: 'signal-snap-1',
      last_seq: 4,
    };

    const unsubscribe = socketsRuntime.standardSocketClient.subscribe({
      lineage,
      onEvent,
      onSubscribed,
    });

    expect(controls).toHaveLength(1);
    const firstSocket = controls[0];

    await waitFor(() => {
      expect(firstSocket.socket.emit).toHaveBeenCalledWith(
        'subscribe',
        expect.objectContaining({
          surface: 'signal',
          stream_id: 'signal-main',
          snapshot_revision: 'signal-snap-1',
          resume_from_seq: 4,
        }),
        expect.any(Function),
      );
    });

    const firstSubscribeAck = firstSocket.socket.emit.mock.calls.filter(
      ([event]) => event === 'subscribe',
    ).at(-1)?.[2];
    firstSubscribeAck?.({
      accepted: true,
      contract_version: 2,
      surface: 'signal',
      profile: 'default',
      surface_query_key: 'signal|profile=default',
      stream_id: 'signal-main',
      snapshot_revision: 'signal-snap-1',
      accepted_start_seq: 4,
      last_seq: 4,
    });
    expect(onSubscribed).toHaveBeenCalledTimes(1);

    socketsRuntime.disconnectSocket();
    expect(firstSocket.socket.removeAllListeners).toHaveBeenCalledTimes(1);

    socketsRuntime.connectSocket();
    expect(controls).toHaveLength(2);

    const secondSocket = controls[1];

    await waitFor(() => {
      expect(secondSocket.socket.emit).toHaveBeenCalledWith(
        'subscribe',
        expect.objectContaining({
          surface: 'signal',
          stream_id: 'signal-main',
          snapshot_revision: 'signal-snap-1',
          resume_from_seq: 4,
        }),
        expect.any(Function),
      );
    });

    const secondSubscribeAck = secondSocket.socket.emit.mock.calls.filter(
      ([event]) => event === 'subscribe',
    ).at(-1)?.[2];
    secondSubscribeAck?.({
      accepted: true,
      contract_version: 2,
      surface: 'signal',
      profile: 'default',
      surface_query_key: 'signal|profile=default',
      stream_id: 'signal-main',
      snapshot_revision: 'signal-snap-1',
      accepted_start_seq: 4,
      last_seq: 4,
    });
    expect(onSubscribed).toHaveBeenCalledTimes(2);

    secondSocket.emit('realtime_event', {
      contract_version: 2,
      surface: 'signal',
      profile: 'default',
      stream_id: 'signal-main',
      snapshot_revision: 'signal-snap-1',
      kind: 'delta_batch',
      seq: 5,
      server_ts_ms: 1_700_000_000_005,
      payload: { signals: [] },
    });

    expect(onEvent).toHaveBeenCalledWith(expect.objectContaining({
      kind: 'delta_batch',
      seq: 5,
    }));

    unsubscribe();
  });
});

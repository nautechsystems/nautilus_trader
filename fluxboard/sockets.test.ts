import { beforeEach, describe, expect, it, vi } from 'vitest';

type EventHandler = (...args: any[]) => void;

type FakeSocketControl = {
  socket: any;
  emit: (event: string, ...args: any[]) => void;
  reconnect: ReturnType<typeof vi.fn>;
  transportClose: ReturnType<typeof vi.fn>;
  connect: ReturnType<typeof vi.fn>;
  disconnect: ReturnType<typeof vi.fn>;
};

type IoCallRecord = {
  url: string;
  options: Record<string, any>;
  control: FakeSocketControl;
};

const ioMock = vi.fn();
const ioCalls: IoCallRecord[] = [];
const bumpGlobalResyncMock = vi.fn();

vi.mock('socket.io-client', () => ({
  io: ioMock,
}));

vi.mock('./stores', () => ({
  bumpGlobalResync: bumpGlobalResyncMock,
}));

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

  const reconnect = vi.fn();
  const transportClose = vi.fn();

  const socket: any = {
    id: `sock-${Math.random().toString(16).slice(2)}`,
    connected: false,
    io: {
      reconnect,
      engine: {
        transport: {
          close: transportClose,
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
    reconnect,
    transportClose,
    connect: socket.connect,
    disconnect: socket.disconnect,
  };
}

beforeEach(() => {
  vi.unstubAllEnvs();
  (window.location as any).pathname = '/';
  delete (window as any).__FLUXBOARD_RUNTIME_CONFIG__;
  vi.resetModules();
  ioMock.mockReset();
  ioCalls.length = 0;
  bumpGlobalResyncMock.mockReset();

  ioMock.mockImplementation((url: string = '', options: Record<string, any> = {}) => {
    const control = createFakeSocket();
    ioCalls.push({ url, options, control });
    return control.socket;
  });
});

describe('sockets status state machine', () => {
  it('starts idle and becomes connecting when getSocket creates the client', async () => {
    const sockets = await import('./sockets');

    expect(sockets.getSocketStatus()).toBe(sockets.SocketConnectionStatus.IDLE);

    sockets.getSocket();

    expect(ioMock).toHaveBeenCalledTimes(1);
    expect(ioCalls[0].url).toBe('');
    expect(ioCalls[0].options.query?.profile).toBe('default');
    expect(sockets.getSocketStatus()).toBe(sockets.SocketConnectionStatus.CONNECTING);
  });

  it('passes tokenmm profile in socket query when path is tokenmm surface', async () => {
    (window.location as any).pathname = '/tokenmm/trades';
    const sockets = await import('./sockets');

    sockets.getSocket();

    expect(ioMock).toHaveBeenCalledTimes(1);
    expect(ioCalls[0].url).toBe('');
    expect(ioCalls[0].options.query?.profile).toBe('tokenmm');
  });

  it('passes equities profile in socket query when path is equities surface', async () => {
    (window.location as any).pathname = '/equities/trades';
    const sockets = await import('./sockets');

    sockets.getSocket();

    expect(ioMock).toHaveBeenCalledTimes(1);
    expect(ioCalls[0].url).toBe('');
    expect(ioCalls[0].options.query?.profile).toBe('equities');
  });

  it('uses runtime-configured equities socket path override when path is equities surface', async () => {
    (window.location as any).pathname = '/equities/signal';
    (window as any).__FLUXBOARD_RUNTIME_CONFIG__ = {
      socketPaths: {
        equities: '/equities/socket.io',
      },
    };
    const sockets = await import('./sockets');

    sockets.getSocket();

    expect(ioMock).toHaveBeenCalledTimes(1);
    expect(ioCalls[0].url).toBe('');
    expect(ioCalls[0].options.path).toBe('/equities/socket.io');
    expect(ioCalls[0].options.query?.profile).toBe('equities');
  });

  it('respects explicit VITE_BACKEND_URL override', async () => {
    vi.stubEnv('VITE_BACKEND_URL', 'http://127.0.0.1:5022');
    const sockets = await import('./sockets');

    sockets.getSocket();

    expect(ioMock).toHaveBeenCalledTimes(1);
    expect(ioCalls[0].url).toBe('http://127.0.0.1:5022');
    vi.unstubAllEnvs();
  });

  it('records explicit disconnect intent and blocks auto-connect on accidental lazy re-create', async () => {
    const sockets = await import('./sockets');

    sockets.getSocket();
    ioCalls[0].control.emit('connect');
    expect(sockets.getSocketStatus()).toBe(sockets.SocketConnectionStatus.CONNECTED);

    sockets.disconnectSocket();

    expect(ioCalls[0].control.reconnect).toHaveBeenCalledWith(false);
    expect(ioCalls[0].control.transportClose).toHaveBeenCalledTimes(1);
    expect(ioCalls[0].control.disconnect).toHaveBeenCalledTimes(1);
    expect(sockets.getSocketStatus()).toBe(sockets.SocketConnectionStatus.DISCONNECT_REQUESTED);

    // Accessing the lazy socket export should not auto-connect while explicitly disconnected.
    void (sockets.socket as any).connected;

    expect(ioMock).toHaveBeenCalledTimes(2);
    expect(ioCalls[1].options.autoConnect).toBe(false);
    expect(sockets.getSocketStatus()).toBe(sockets.SocketConnectionStatus.DISCONNECT_REQUESTED);

    ioCalls[1].control.emit('reconnect_attempt', 1);
    expect(ioCalls[1].control.disconnect).toHaveBeenCalledTimes(1);
    expect(sockets.getSocketStatus()).toBe(sockets.SocketConnectionStatus.DISCONNECT_REQUESTED);
  });

  it('supports explicit reconnect after disconnect request', async () => {
    const sockets = await import('./sockets');

    sockets.getSocket();
    sockets.disconnectSocket();

    expect(sockets.getSocketStatus()).toBe(sockets.SocketConnectionStatus.DISCONNECT_REQUESTED);

    sockets.connectSocket();

    expect(ioMock).toHaveBeenCalledTimes(2);
    expect(ioCalls[1].control.reconnect).toHaveBeenCalledWith(true);
    expect(ioCalls[1].control.connect).toHaveBeenCalledTimes(1);
    expect(sockets.getSocketStatus()).toBe(sockets.SocketConnectionStatus.CONNECTING);

    ioCalls[1].control.emit('connect');
    expect(sockets.getSocketStatus()).toBe(sockets.SocketConnectionStatus.CONNECTED);
  });

  it('re-attaches shared standard socket subscriptions after the socket instance is recreated', async () => {
    const sockets = await import('./sockets');
    let currentResumeFromSeq = 3;
    const onSubscribed = vi.fn();
    const onEvent = vi.fn();

    sockets.getSocket();
    ioCalls[0].control.socket.connected = true;

    const unsubscribe = sockets.standardSocketClient.subscribe({
      lineage: {
        contract_version: 2,
        surface: 'signal',
        profile: 'default',
        surface_query_key: 'signal|profile=default',
        stream_id: 'signal-main',
        snapshot_revision: 'snap-1',
        last_seq: 3,
      },
      resumeFromSeq: () => currentResumeFromSeq,
      onEvent,
      onSubscribed,
    });

    const firstSubscribeCall = ioCalls[0].control.socket.emit.mock.calls.find(
      ([event]) => event === 'subscribe',
    );
    expect(firstSubscribeCall).toBeDefined();
    const firstAck = firstSubscribeCall?.[2];
    firstAck?.({
      accepted: true,
      contract_version: 2,
      surface: 'signal',
      profile: 'default',
      surface_query_key: 'signal|profile=default',
      stream_id: 'signal-main',
      snapshot_revision: 'snap-1',
      accepted_start_seq: 3,
      last_seq: 3,
    });
    expect(onSubscribed).toHaveBeenCalledTimes(1);

    sockets.disconnectSocket();
    currentResumeFromSeq = 6;
    sockets.connectSocket();

    expect(ioMock).toHaveBeenCalledTimes(2);
    ioCalls[1].control.socket.connected = true;
    ioCalls[1].control.emit('connect');

    const secondSubscribeCall = ioCalls[1].control.socket.emit.mock.calls.find(
      ([event]) => event === 'subscribe',
    );
    expect(secondSubscribeCall).toBeDefined();
    expect(secondSubscribeCall?.[1]).toEqual(expect.objectContaining({
      surface: 'signal',
      stream_id: 'signal-main',
      snapshot_revision: 'snap-1',
      resume_from_seq: 6,
    }));

    const secondAck = secondSubscribeCall?.[2];
    secondAck?.({
      accepted: true,
      contract_version: 2,
      surface: 'signal',
      profile: 'default',
      surface_query_key: 'signal|profile=default',
      stream_id: 'signal-main',
      snapshot_revision: 'snap-1',
      accepted_start_seq: 6,
      last_seq: 6,
    });
    expect(onSubscribed).toHaveBeenCalledTimes(2);

    ioCalls[1].control.emit('realtime_event', {
      contract_version: 2,
      surface: 'signal',
      profile: 'default',
      stream_id: 'signal-main',
      snapshot_revision: 'snap-1',
      kind: 'delta_batch',
      seq: 7,
      server_ts_ms: 1_700_000_000_007,
      payload: { signals: [] },
    });

    expect(onEvent).toHaveBeenCalledTimes(1);

    unsubscribe();
    expect(ioCalls[1].control.socket.emit).toHaveBeenCalledWith('unsubscribe', { surface: 'signal' });
  });

  it('tracks reconnect attempts as reconnecting when reconnect is allowed', async () => {
    const sockets = await import('./sockets');

    sockets.getSocket();
    ioCalls[0].control.emit('connect');
    expect(sockets.getSocketStatus()).toBe(sockets.SocketConnectionStatus.CONNECTED);

    ioCalls[0].control.emit('reconnect_attempt', 1);

    expect(sockets.getSocketStatus()).toBe(sockets.SocketConnectionStatus.RECONNECTING);
  });

  it('debounces global resync bumps across rapid reconnects', async () => {
    const sockets = await import('./sockets');

    sockets.getSocket();

    ioCalls[0].control.emit('connect');
    expect(bumpGlobalResyncMock).not.toHaveBeenCalled();

    ioCalls[0].control.emit('disconnect', 'transport close');
    ioCalls[0].control.emit('connect');
    expect(bumpGlobalResyncMock).toHaveBeenCalledTimes(1);

    ioCalls[0].control.emit('disconnect', 'transport close');
    ioCalls[0].control.emit('connect');
    expect(bumpGlobalResyncMock).toHaveBeenCalledTimes(1);
  });

  it('re-subscribes active standard subscriptions after socket reconnects', async () => {
    const sockets = await import('./sockets');
    const control = createFakeSocket();
    control.socket.connected = true;
    const client = sockets.createStandardSocketClient(control.socket);
    const onSubscribed = vi.fn();
    const onEvent = vi.fn();
    let currentResumeFromSeq = 7;
    const lineage = {
      contract_version: 2,
      surface: 'trades',
      profile: 'default',
      surface_query_key: 'trades|profile=default',
      stream_id: 'trades-main',
      snapshot_revision: 'snap-1',
      last_seq: currentResumeFromSeq,
    };

    const unsubscribe = client.subscribe({
      lineage,
      resumeFromSeq: () => currentResumeFromSeq,
      onEvent,
      onSubscribed,
    });

    expect(control.socket.emit).toHaveBeenCalledWith(
      'subscribe',
      expect.objectContaining({
        contract_version: 2,
        surface: 'trades',
        stream_id: 'trades-main',
        snapshot_revision: 'snap-1',
        resume_from_seq: 7,
      }),
      expect.any(Function),
    );
    const firstAck = control.socket.emit.mock.calls[0]?.[2];
    expect(typeof firstAck).toBe('function');
    firstAck({
      accepted: true,
      contract_version: 2,
      surface: 'trades',
      profile: 'default',
      surface_query_key: 'trades|profile=default',
      stream_id: 'trades-main',
      snapshot_revision: 'snap-1',
      accepted_start_seq: 7,
      last_seq: 7,
    });
    expect(onSubscribed).toHaveBeenCalledTimes(1);

    control.emit('disconnect', 'transport close');
    currentResumeFromSeq = 8;
    control.emit('connect');

    expect(control.socket.emit).toHaveBeenCalledTimes(2);
    expect(control.socket.emit).toHaveBeenLastCalledWith(
      'subscribe',
      expect.objectContaining({
        surface: 'trades',
        stream_id: 'trades-main',
        snapshot_revision: 'snap-1',
        resume_from_seq: 8,
      }),
      expect.any(Function),
    );
    const secondAck = control.socket.emit.mock.calls[1]?.[2];
    expect(typeof secondAck).toBe('function');
    secondAck({
      accepted: true,
      contract_version: 2,
      surface: 'trades',
      profile: 'default',
      surface_query_key: 'trades|profile=default',
      stream_id: 'trades-main',
      snapshot_revision: 'snap-1',
      accepted_start_seq: 8,
      last_seq: 8,
    });
    expect(onSubscribed).toHaveBeenCalledTimes(2);

    control.emit('realtime_event', {
      contract_version: 2,
      surface: 'trades',
      profile: 'default',
      stream_id: 'trades-main',
      snapshot_revision: 'snap-1',
      kind: 'delta_batch',
      seq: 8,
      server_ts_ms: 1_700_000_000_008,
      payload: { trades: [] },
    });
    expect(onEvent).toHaveBeenCalledTimes(1);

    unsubscribe();
    expect(control.socket.emit).toHaveBeenCalledWith('unsubscribe', { surface: 'trades' });
  });

  it('keeps same-surface subscriptions isolated until the last consumer unsubscribes', async () => {
    const sockets = await import('./sockets');
    const control = createFakeSocket();
    control.socket.connected = true;
    const client = sockets.createStandardSocketClient(control.socket);
    const lineage = {
      contract_version: 2,
      surface: 'signal',
      profile: 'default',
      surface_query_key: 'signal|profile=default',
      stream_id: 'signal-main',
      snapshot_revision: 'snap-1',
      last_seq: 3,
    };

    const unsubscribeA = client.subscribe({ lineage, onEvent: vi.fn() });
    const ackA = control.socket.emit.mock.calls[0]?.[2];
    ackA?.({
      accepted: true,
      contract_version: 2,
      surface: 'signal',
      profile: 'default',
      surface_query_key: 'signal|profile=default',
      stream_id: 'signal-main',
      snapshot_revision: 'snap-1',
      accepted_start_seq: 3,
      last_seq: 3,
    });

    const unsubscribeB = client.subscribe({ lineage, onEvent: vi.fn() });
    const ackB = control.socket.emit.mock.calls[1]?.[2];
    ackB?.({
      accepted: true,
      contract_version: 2,
      surface: 'signal',
      profile: 'default',
      surface_query_key: 'signal|profile=default',
      stream_id: 'signal-main',
      snapshot_revision: 'snap-1',
      accepted_start_seq: 3,
      last_seq: 3,
    });

    unsubscribeA();
    expect(control.socket.emit).not.toHaveBeenCalledWith('unsubscribe', { surface: 'signal' });

    unsubscribeB();
    expect(control.socket.emit).toHaveBeenCalledWith('unsubscribe', { surface: 'signal' });
  });

  it('accepts subscribe acks and realtime events when snapshot revisions differ only by type', async () => {
    const sockets = await import('./sockets');
    const control = createFakeSocket();
    control.socket.connected = true;
    const client = sockets.createStandardSocketClient(control.socket);
    const onSubscribed = vi.fn();
    const onEvent = vi.fn();

    client.subscribe({
      lineage: {
        contract_version: 2,
        surface: 'trades',
        profile: 'default',
        surface_query_key: 'trades|profile=default',
        stream_id: 'trades-main',
        snapshot_revision: 17,
        last_seq: 4,
      },
      onEvent,
      onSubscribed,
    });

    const ack = control.socket.emit.mock.calls[0]?.[2];
    ack?.({
      accepted: true,
      contract_version: 2,
      surface: 'trades',
      profile: 'default',
      surface_query_key: 'trades|profile=default',
      stream_id: 'trades-main',
      snapshot_revision: '17',
      accepted_start_seq: 4,
      last_seq: 4,
    });

    expect(onSubscribed).toHaveBeenCalledTimes(1);

    control.emit('realtime_event', {
      contract_version: 2,
      surface: 'trades',
      profile: 'default',
      stream_id: 'trades-main',
      snapshot_revision: '17',
      kind: 'delta_batch',
      seq: 5,
      server_ts_ms: 1_700_000_000_005,
      payload: { trades: [] },
    });

    expect(onEvent).toHaveBeenCalledTimes(1);
  });

  it('emits unsubscribe when an accepted subscribe ack mismatches the requested lineage', async () => {
    const sockets = await import('./sockets');
    const control = createFakeSocket();
    control.socket.connected = true;
    const client = sockets.createStandardSocketClient(control.socket);
    const onFailure = vi.fn();

    client.subscribe({
      lineage: {
        contract_version: 2,
        surface: 'signal',
        profile: 'default',
        surface_query_key: 'signal|profile=default',
        stream_id: 'signal-main',
        snapshot_revision: 'snap-1',
        last_seq: 3,
      },
      onEvent: vi.fn(),
      onFailure,
    });

    const ack = control.socket.emit.mock.calls[0]?.[2];
    ack?.({
      accepted: true,
      contract_version: 2,
      surface: 'signal',
      profile: 'default',
      surface_query_key: 'signal|profile=default',
      stream_id: 'signal-main',
      snapshot_revision: 'other-snap',
      accepted_start_seq: 3,
      last_seq: 3,
    });

    expect(onFailure).toHaveBeenCalledWith(expect.objectContaining({
      type: 'lineage_mismatch',
      reason: 'ack_lineage_mismatch',
    }));
    expect(control.socket.emit).toHaveBeenCalledWith('unsubscribe', { surface: 'signal' });
  });

  it('emits unsubscribe when an accepted subscribe ack changes the accepted start seq', async () => {
    const sockets = await import('./sockets');
    const control = createFakeSocket();
    control.socket.connected = true;
    const client = sockets.createStandardSocketClient(control.socket);
    const onFailure = vi.fn();

    client.subscribe({
      lineage: {
        contract_version: 2,
        surface: 'trades',
        profile: 'default',
        surface_query_key: 'trades|profile=default',
        stream_id: 'trades-main',
        snapshot_revision: 'snap-1',
        last_seq: 4,
      },
      onEvent: vi.fn(),
      onFailure,
    });

    const ack = control.socket.emit.mock.calls[0]?.[2];
    ack?.({
      accepted: true,
      contract_version: 2,
      surface: 'trades',
      profile: 'default',
      surface_query_key: 'trades|profile=default',
      stream_id: 'trades-main',
      snapshot_revision: 'snap-1',
      accepted_start_seq: 9,
      last_seq: 9,
    });

    expect(onFailure).toHaveBeenCalledWith(expect.objectContaining({
      type: 'lineage_mismatch',
      reason: 'accepted_start_seq_mismatch',
    }));
    expect(control.socket.emit).toHaveBeenCalledWith('unsubscribe', { surface: 'trades' });
  });

  it('emits unsubscribe and removes the local subscription when trades recovery is required', async () => {
    const sockets = await import('./sockets');
    const control = createFakeSocket();
    control.socket.connected = true;
    const client = sockets.createStandardSocketClient(control.socket);
    const onFailure = vi.fn();

    client.subscribe({
      lineage: {
        contract_version: 2,
        surface: 'trades',
        profile: 'default',
        surface_query_key: 'trades|profile=default',
        stream_id: 'trades-main',
        snapshot_revision: 'snap-1',
        last_seq: 4,
      },
      onEvent: vi.fn(),
      onFailure,
    });

    const ack = control.socket.emit.mock.calls[0]?.[2];
    ack?.({
      accepted: true,
      contract_version: 2,
      surface: 'trades',
      profile: 'default',
      surface_query_key: 'trades|profile=default',
      stream_id: 'trades-main',
      snapshot_revision: 'snap-1',
      accepted_start_seq: 4,
      last_seq: 4,
    });

    control.socket.emit.mockClear();

    control.emit('realtime_event', {
      contract_version: 2,
      surface: 'trades',
      profile: 'default',
      stream_id: 'trades-main',
      snapshot_revision: 'snap-1',
      kind: 'recovery_required',
      seq: 5,
      server_ts_ms: 1_700_000_000_005,
      reason: 'trade_gap',
      payload: {},
    });

    expect(onFailure).toHaveBeenCalledWith(expect.objectContaining({
      type: 'recovery_required',
      reason: 'trade_gap',
    }));
    expect(control.socket.emit).toHaveBeenCalledWith('unsubscribe', { surface: 'trades' });

    control.socket.emit.mockClear();
    control.emit('disconnect', 'transport close');
    control.emit('connect');

    expect(control.socket.emit).not.toHaveBeenCalledWith(
      'subscribe',
      expect.anything(),
      expect.any(Function),
    );
  });

  it('ignores stale subscribe acks from a superseded reconnect attempt', async () => {
    const sockets = await import('./sockets');
    const control = createFakeSocket();
    control.socket.connected = true;
    const client = sockets.createStandardSocketClient(control.socket);
    const onFailure = vi.fn();
    const onSubscribed = vi.fn();
    let currentResumeFromSeq = 3;

    client.subscribe({
      lineage: {
        contract_version: 2,
        surface: 'signal',
        profile: 'default',
        surface_query_key: 'signal|profile=default',
        stream_id: 'signal-main',
        snapshot_revision: 'snap-1',
        last_seq: 3,
      },
      resumeFromSeq: () => currentResumeFromSeq,
      onEvent: vi.fn(),
      onFailure,
      onSubscribed,
    });

    const firstAck = control.socket.emit.mock.calls[0]?.[2];
    expect(typeof firstAck).toBe('function');

    control.emit('disconnect', 'transport close');
    currentResumeFromSeq = 5;
    control.emit('connect');

    const secondAck = control.socket.emit.mock.calls[1]?.[2];
    expect(typeof secondAck).toBe('function');

    firstAck?.({
      accepted: true,
      contract_version: 2,
      surface: 'signal',
      profile: 'default',
      surface_query_key: 'signal|profile=default',
      stream_id: 'signal-main',
      snapshot_revision: 'snap-1',
      accepted_start_seq: 3,
      last_seq: 3,
    });

    expect(onFailure).not.toHaveBeenCalled();
    expect(control.socket.emit).not.toHaveBeenCalledWith('unsubscribe', { surface: 'signal' });

    secondAck?.({
      accepted: true,
      contract_version: 2,
      surface: 'signal',
      profile: 'default',
      surface_query_key: 'signal|profile=default',
      stream_id: 'signal-main',
      snapshot_revision: 'snap-1',
      accepted_start_seq: 5,
      last_seq: 5,
    });

    expect(onSubscribed).toHaveBeenCalledTimes(1);
    expect(onFailure).not.toHaveBeenCalled();
  });
});

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
});

// Socket.IO client for real-time updates

import { io, Socket } from 'socket.io-client';
import { bumpGlobalResync } from './stores';
import { resolvePathnameProfile } from './config/uiProfiles';

let socketInstance: Socket | null = null;
let disconnectRequested = false;
let hasConnectedOnce = false;
let lastResyncBumpAt = 0;
let currentSocketProfile = 'default';
const RESYNC_BUMP_DEBOUNCE_MS = 2000;

export enum SocketConnectionStatus {
  IDLE = 'idle',
  CONNECTING = 'connecting',
  CONNECTED = 'connected',
  RECONNECTING = 'reconnecting',
  DISCONNECTED = 'disconnected',
  DISCONNECT_REQUESTED = 'disconnect_requested',
  ERROR = 'error',
}

let socketStatus: SocketConnectionStatus = SocketConnectionStatus.IDLE;

const getTestSocketFactory = (): (() => Socket) | null => {
  if (typeof window === 'undefined') {
    return null;
  }
  const factory = (window as any).__fluxboardTestSocketFactory;
  return typeof factory === 'function' ? factory as () => Socket : null;
};

const setSocketStatus = (status: SocketConnectionStatus): void => {
  socketStatus = status;
};

export const getSocketStatus = (): SocketConnectionStatus => socketStatus;

const getPathProfile = (): string => {
  if (typeof window === 'undefined') {
    return 'default';
  }
  return resolvePathnameProfile(window.location?.pathname);
};

const syncSocketProfile = (): void => {
  if (!socketInstance) {
    return;
  }
  const nextProfile = getPathProfile();
  if (nextProfile === currentSocketProfile) {
    return;
  }
  currentSocketProfile = nextProfile;
  if (socketInstance.connected) {
    socketInstance.emit('set_profile', { profile: nextProfile });
  }
};

export const getSocket = (): Socket => {
  if (!socketInstance) {
    // Default to same-origin; Vite dev proxy should forward /socket.io to FluxAPI.
    // Allow explicit override for cross-origin development setups.
    const configuredBackendUrl = String(import.meta.env.VITE_BACKEND_URL || '').trim();
    const backendUrl = configuredBackendUrl === '/' ? '' : configuredBackendUrl;
    currentSocketProfile = getPathProfile();
    const testSocketFactory = getTestSocketFactory();
    const usingTestSocket = Boolean(testSocketFactory);

    const autoConnect = !disconnectRequested;
    setSocketStatus(
      autoConnect
        ? SocketConnectionStatus.CONNECTING
        : SocketConnectionStatus.DISCONNECT_REQUESTED,
    );

    socketInstance = usingTestSocket
      ? testSocketFactory!()
      : io(backendUrl, {
          path: '/socket.io',  // NO trailing slash
          // Server runs with threading mode; disable WS upgrade to avoid issues
          transports: ['polling'],
          // Allow cookies/headers when hosted on a different port during dev
          withCredentials: true,
          reconnection: true,
          // Limit reconnection attempts to prevent infinite loops
          reconnectionAttempts: 10,  // Changed from Infinity
          reconnectionDelay: 1000,   // Start with 1s delay
          reconnectionDelayMax: 10000, // Max 10s delay
          timeout: 20000,
          // Add exponential backoff multiplier
          randomizationFactor: 0.5,
          // Prevent accidental reconnect when explicit disconnect was requested.
          autoConnect,
          query: {
            profile: currentSocketProfile,
          },
        });

    // Prevent auto-reconnection when explicitly disconnected (e.g., on PnL page)
    const reconnectHandler = () => {
      if (disconnectRequested) {
        setSocketStatus(SocketConnectionStatus.DISCONNECT_REQUESTED);
        socketInstance?.disconnect();
        return;
      }
      setSocketStatus(SocketConnectionStatus.RECONNECTING);
    };
    socketInstance.on('reconnect_attempt', reconnectHandler);

    socketInstance.on('connect', () => {
      if (disconnectRequested) {
        setSocketStatus(SocketConnectionStatus.DISCONNECT_REQUESTED);
        socketInstance?.disconnect();
        return;
      }

      const isReconnect = hasConnectedOnce;
      hasConnectedOnce = true;
      if (isReconnect) {
        const now = Date.now();
        if (now - lastResyncBumpAt >= RESYNC_BUMP_DEBOUNCE_MS) {
          lastResyncBumpAt = now;
          bumpGlobalResync('socket-reconnect');
        }
      }
      disconnectRequested = false;
      setSocketStatus(SocketConnectionStatus.CONNECTED);
      const activeProfile = getPathProfile();
      currentSocketProfile = activeProfile;
      socketInstance?.emit('set_profile', { profile: activeProfile });

      if (import.meta.env && import.meta.env.DEV) {
        console.log('[socket] connected', socketInstance?.id);
      }
    });

    socketInstance.on('disconnect', (reason) => {
      setSocketStatus(
        disconnectRequested
          ? SocketConnectionStatus.DISCONNECT_REQUESTED
          : SocketConnectionStatus.DISCONNECTED,
      );

      if (import.meta.env && import.meta.env.DEV) {
        console.log('[socket] disconnected', reason);
      }
    });

    socketInstance.on('connect_error', (err) => {
      if (disconnectRequested) {
        setSocketStatus(SocketConnectionStatus.DISCONNECT_REQUESTED);
      } else {
        setSocketStatus(SocketConnectionStatus.ERROR);
      }

      if (import.meta.env && import.meta.env.DEV) {
        console.error('[socket] connection error', err.message);
      }
    });

    socketInstance.on('reconnect_failed', () => {
      setSocketStatus(
        disconnectRequested
          ? SocketConnectionStatus.DISCONNECT_REQUESTED
          : SocketConnectionStatus.DISCONNECTED,
      );

      if (import.meta.env && import.meta.env.DEV) {
        console.warn('[socket] reconnect failed after max attempts');
      }
    });

    // Debug logging in development
    if (import.meta.env && import.meta.env.DEV) {
      socketInstance.on('reconnect_attempt', (attempt) => {
        console.log('[socket] reconnect attempt', attempt);
        if (disconnectRequested) {
          console.log('[socket] reconnect blocked - disconnect requested');
        }
      });
    }

    if (usingTestSocket && autoConnect && !socketInstance.connected) {
      socketInstance.connect();
    }
  }

  return socketInstance;
};

/**
 * Disconnect socket.io (useful for pages that don't need real-time data like PnL).
 * Prevents auto-reconnection until explicitly reconnected.
 * Force disconnects even if already disconnected to ensure clean state.
 *
 * CRITICAL: Closes the underlying transport connection to free server-side threads.
 * This is essential for Flask-SocketIO threading mode where each connection holds a thread.
 */
export const disconnectSocket = (): void => {
  disconnectRequested = true;
  setSocketStatus(SocketConnectionStatus.DISCONNECT_REQUESTED);

  if (socketInstance) {
    // Disable auto-reconnection FIRST before disconnecting
    socketInstance.io.reconnect(false);

    // CRITICAL: Close underlying transport connection to free server-side thread
    // This ensures Flask-SocketIO releases the thread immediately
    try {
      if (socketInstance.io?.engine?.transport) {
        socketInstance.io.engine.transport.close();
      }
    } catch (e) {
      // Transport may already be closed, ignore
      if (import.meta.env?.DEV) {
        console.warn('[socket] Transport close error (ignored):', e);
      }
    }

    // Force disconnect (works even if already disconnected)
    socketInstance.disconnect();
    // Remove all listeners to prevent any reconnection attempts
    socketInstance.removeAllListeners();

    // Clear instance to force recreation on next connect
    socketInstance = null;
    // Also clear the exported socket reference
    _socketExport = null;

    if (import.meta.env?.DEV) {
      console.log('[socket] Force disconnected for PnL page - connection closed');
    }
  }
};

/**
 * Connect socket.io (reconnects if previously disconnected).
 * Re-enables auto-reconnection.
 * Creates new socket instance if it was cleared.
 */
export const connectSocket = (): void => {
  disconnectRequested = false;

  // If socket was cleared, recreate it
  if (!socketInstance) {
    socketInstance = getSocket();
  }
  // Re-enable auto-reconnection
  socketInstance.io.reconnect(true);

  if (!socketInstance.connected) {
    setSocketStatus(SocketConnectionStatus.CONNECTING);
  } else {
    setSocketStatus(SocketConnectionStatus.CONNECTED);
  }

  if (!socketInstance.connected) {
    socketInstance.connect();
  } else {
    syncSocketProfile();
  }
};

// Lazy export - socket is created on first access via getter
// This allows PnL page to disconnect before socket is created
let _socketExport: Socket | null = null;

function getSocketExport(): Socket {
  if (!_socketExport) {
    _socketExport = getSocket();
  } else {
    syncSocketProfile();
  }
  return _socketExport;
}

// Export socket as a getter to allow PnL to disconnect before creation
export const socket = new Proxy({} as Socket, {
  get(_target, prop) {
    const instance = getSocketExport();
    const value = (instance as any)[prop];
    // Bind methods to maintain 'this' context
    if (typeof value === 'function') {
      return value.bind(instance);
    }
    return value;
  },
});

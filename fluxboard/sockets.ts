// Socket.IO client for real-time updates

import { io, Socket } from 'socket.io-client';
import { bumpGlobalResync } from './stores';
import { resolvePathnameProfile, type PathProfile } from './config/uiProfiles';
import type { RealtimeSnapshotLineage } from './types';

let socketInstance: Socket | null = null;
let disconnectRequested = false;
let hasConnectedOnce = false;
let lastResyncBumpAt = 0;
let currentSocketProfile = 'default';
const RESYNC_BUMP_DEBOUNCE_MS = 2000;
const DEFAULT_SOCKET_PATH = '/socket.io';

type FluxboardRuntimeConfig = {
  socketPaths?: Partial<Record<PathProfile, string>>;
};

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

export type StandardRealtimeSubscribePayload = {
  contract_version: number;
  surface: string;
  profile: string;
  surface_query_key: string;
  stream_id: string;
  snapshot_revision: number | string;
  resume_from_seq: number;
};

export type StandardRealtimeSubscribeAck = {
  accepted: boolean;
  contract_version?: number;
  surface?: string;
  profile?: string;
  reason?: string;
  surface_query_key?: string;
  stream_id?: string;
  snapshot_revision?: number | string;
  accepted_start_seq?: number;
  last_seq?: number;
  capabilities?: Record<string, unknown>;
  requested_resume_from_seq?: number;
};

export type StandardRealtimeUnsubscribeAck = {
  ok: boolean;
  surface?: string | null;
};

export type StandardRealtimeEvent = {
  contract_version: number;
  surface: string;
  stream_id: string;
  profile: string;
  kind: string;
  seq: number;
  snapshot_revision: number | string;
  server_ts_ms: number;
  reason?: string;
  payload?: Record<string, unknown>;
};

export type StandardSocketEventKind =
  | 'delta_batch'
  | 'heartbeat'
  | 'invalidate'
  | 'recovery_required'
  | string;

export type StandardSocketSubscribeRequest = Pick<
  RealtimeSnapshotLineage,
  'contract_version' | 'surface' | 'profile' | 'surface_query_key' | 'stream_id' | 'snapshot_revision'
> & {
  resume_from_seq: number;
};

export type StandardSocketSubscribeAck = {
  accepted: boolean;
  contract_version?: number;
  surface?: string;
  profile?: string;
  surface_query_key?: string;
  stream_id?: string;
  snapshot_revision?: number | string;
  accepted_start_seq?: number;
  last_seq?: number;
  requested_resume_from_seq?: number;
  capabilities?: RealtimeSnapshotLineage['capabilities'];
  reason?: string;
};

export type StandardSocketEventEnvelope<TPayload = unknown> = {
  contract_version: number;
  surface: string;
  stream_id: string;
  profile: string;
  kind: StandardSocketEventKind;
  seq: number;
  snapshot_revision: number | string;
  server_ts_ms: number;
  reason?: string;
  payload?: TPayload;
};

export type StandardSocketFailure =
  | {
      type: 'subscribe_rejected';
      reason: string;
      requested: StandardSocketSubscribeRequest;
      ack?: StandardSocketSubscribeAck;
    }
  | {
      type: 'lineage_mismatch';
      reason: string;
      requested: StandardSocketSubscribeRequest;
      ack?: StandardSocketSubscribeAck;
    }
  | {
      type: 'recovery_required';
      reason: string;
      requested: StandardSocketSubscribeRequest;
      event: StandardSocketEventEnvelope;
    };

export interface StandardSocketLike {
  connected?: boolean;
  on: (event: string, handler: (payload?: any) => void) => unknown;
  off: (event: string, handler?: (payload?: any) => void) => unknown;
  emit: (event: string, payload?: any, ack?: (response: any) => void) => unknown;
}

type StandardSocketSubscription<TPayload> = {
  id: number;
  lineage: RealtimeSnapshotLineage;
  request: StandardSocketSubscribeRequest;
  issueId: number;
  resolveResumeFromSeq: () => number;
  onEvent: (event: StandardSocketEventEnvelope<TPayload>) => void;
  onFailure?: (failure: StandardSocketFailure) => void;
  onSubscribed?: (ack: StandardSocketSubscribeAck) => void;
};

export interface StandardSocketClient {
  subscribe: <TPayload = unknown>(options: {
    lineage: RealtimeSnapshotLineage;
    resumeFromSeq?: number | (() => number);
    onEvent: (event: StandardSocketEventEnvelope<TPayload>) => void;
    onFailure?: (failure: StandardSocketFailure) => void;
    onSubscribed?: (ack: StandardSocketSubscribeAck) => void;
  }) => () => void;
  refreshSocketBinding: () => void;
  resetForTests: () => void;
}

type StandardSocketTarget =
  | StandardSocketLike
  | (() => StandardSocketLike | null | undefined);

function buildStandardSubscribeRequest(
  lineage: RealtimeSnapshotLineage,
  resumeFromSeq: number,
): StandardSocketSubscribeRequest {
  return {
    contract_version: lineage.contract_version,
    surface: lineage.surface,
    profile: lineage.profile,
    surface_query_key: lineage.surface_query_key,
    stream_id: lineage.stream_id,
    snapshot_revision: lineage.snapshot_revision,
    resume_from_seq: Math.max(0, Math.trunc(resumeFromSeq)),
  };
}

function normalizeLineageIdentityValue(value: number | string | undefined | null): string | null {
  if (value === undefined || value === null) {
    return null;
  }
  return String(value);
}

function matchesStandardSocketEvent(
  request: StandardSocketSubscribeRequest,
  event: StandardSocketEventEnvelope,
): boolean {
  return (
    event.contract_version === request.contract_version
    && event.surface === request.surface
    && event.profile === request.profile
    && event.stream_id === request.stream_id
    && normalizeLineageIdentityValue(event.snapshot_revision) === normalizeLineageIdentityValue(request.snapshot_revision)
  );
}

function matchesStandardSubscribeAck(
  request: StandardSocketSubscribeRequest,
  ack: StandardSocketSubscribeAck,
): boolean {
  return (
    ack.contract_version === request.contract_version
    && ack.surface === request.surface
    && ack.profile === request.profile
    && ack.surface_query_key === request.surface_query_key
    && ack.stream_id === request.stream_id
    && normalizeLineageIdentityValue(ack.snapshot_revision) === normalizeLineageIdentityValue(request.snapshot_revision)
  );
}

export function createStandardSocketClient(
  targetSocket?: StandardSocketTarget,
): StandardSocketClient {
  const resolveSocket = (): StandardSocketLike => {
    if (typeof targetSocket === 'function') {
      return targetSocket() ?? (socket as unknown as StandardSocketLike);
    }
    return targetSocket ?? (socket as unknown as StandardSocketLike);
  };
  let nextSubscriptionId = 0;
  let boundSocket: StandardSocketLike | null = null;
  let realtimeEventHandler: ((payload?: any) => void) | null = null;
  let connectHandler: (() => void) | null = null;
  const subscriptions = new Map<number, StandardSocketSubscription<any>>();

  const attachSocketHandlers = (nextSocket: StandardSocketLike) => {
    if (realtimeEventHandler) {
      nextSocket.on('realtime_event', realtimeEventHandler);
    }
    if (connectHandler) {
      nextSocket.on('connect', connectHandler);
    }
  };

  const detachSocketHandlersFromBoundSocket = () => {
    if (!boundSocket) {
      return;
    }
    if (realtimeEventHandler) {
      boundSocket.off('realtime_event', realtimeEventHandler);
    }
    if (connectHandler) {
      boundSocket.off('connect', connectHandler);
    }
    boundSocket = null;
  };

  const refreshSocketBinding = (): boolean => {
    if (subscriptions.size === 0) {
      detachSocketHandlersFromBoundSocket();
      return false;
    }
    const nextSocket = resolveSocket();
    if (nextSocket === boundSocket) {
      return false;
    }
    detachSocketHandlersFromBoundSocket();
    boundSocket = nextSocket;
    attachSocketHandlers(nextSocket);
    return true;
  };

  const detachSocketHandlers = () => {
    if (subscriptions.size !== 0) {
      return;
    }
    detachSocketHandlersFromBoundSocket();
    realtimeEventHandler = null;
    connectHandler = null;
  };

  const removeSubscription = (subscriptionId: number): StandardSocketSubscription<any> | undefined => {
    const subscription = subscriptions.get(subscriptionId);
    if (!subscription) {
      return undefined;
    }
    subscriptions.delete(subscriptionId);
    detachSocketHandlers();
    return subscription;
  };

  const emitSurfaceUnsubscribeIfUnused = (
    surface: string,
    socketTarget: StandardSocketLike | null = boundSocket,
  ) => {
    const hasSameSurfaceSubscription = [...subscriptions.values()].some(
      (candidate) => candidate.request.surface === surface,
    );
    if (!hasSameSurfaceSubscription && socketTarget) {
      socketTarget.emit('unsubscribe', { surface });
    }
  };

  const issueSubscribe = (subscription: StandardSocketSubscription<any>) => {
    const issuedRequest = buildStandardSubscribeRequest(
      subscription.lineage,
      subscription.resolveResumeFromSeq(),
    );
    subscription.request = issuedRequest;
    subscription.issueId += 1;
    const issuedRequestId = subscription.issueId;
    (boundSocket ?? resolveSocket()).emit('subscribe', issuedRequest, (response: any) => {
      const activeSubscription = subscriptions.get(subscription.id);
      if (!activeSubscription || activeSubscription.issueId !== issuedRequestId) {
        return;
      }

      const ack = response as StandardSocketSubscribeAck;
      if (!ack?.accepted) {
        removeSubscription(subscription.id);
        subscription.onFailure?.({
          type: 'subscribe_rejected',
          reason: String(ack?.reason ?? 'subscribe_rejected'),
          requested: issuedRequest,
          ack,
        });
        return;
      }

      if (!matchesStandardSubscribeAck(issuedRequest, ack)) {
        const socketTarget = boundSocket;
        removeSubscription(subscription.id);
        emitSurfaceUnsubscribeIfUnused(issuedRequest.surface, socketTarget);
        subscription.onFailure?.({
          type: 'lineage_mismatch',
          reason: 'ack_lineage_mismatch',
          requested: issuedRequest,
          ack,
        });
        return;
      }

      if (
        typeof ack.accepted_start_seq === 'number'
        && ack.accepted_start_seq !== issuedRequest.resume_from_seq
      ) {
        const socketTarget = boundSocket;
        removeSubscription(subscription.id);
        emitSurfaceUnsubscribeIfUnused(issuedRequest.surface, socketTarget);
        subscription.onFailure?.({
          type: 'lineage_mismatch',
          reason: 'accepted_start_seq_mismatch',
          requested: issuedRequest,
          ack,
        });
        return;
      }

      subscription.onSubscribed?.(ack);
    });
  };

  const ensureSocketHandlers = () => {
    if (!realtimeEventHandler) {
      realtimeEventHandler = (payload?: any) => {
      const event = payload as StandardSocketEventEnvelope;
      if (!event || typeof event !== 'object') {
        return;
      }

      for (const subscription of [...subscriptions.values()]) {
        if (!matchesStandardSocketEvent(subscription.request, event)) {
          continue;
        }

        if (event.kind === 'recovery_required') {
          subscription.onFailure?.({
            type: 'recovery_required',
            reason: String(event.reason ?? 'recovery_required'),
            requested: subscription.request,
            event,
          });
          continue;
        }

        subscription.onEvent(event);
      }
    };
    }
    if (!connectHandler) {
      connectHandler = () => {
        for (const subscription of subscriptions.values()) {
          issueSubscribe(subscription);
        }
      };
    }
    refreshSocketBinding();
  };

  return {
    subscribe<TPayload = unknown>(options: {
      lineage: RealtimeSnapshotLineage;
      resumeFromSeq?: number | (() => number);
      onEvent: (event: StandardSocketEventEnvelope<TPayload>) => void;
      onFailure?: (failure: StandardSocketFailure) => void;
      onSubscribed?: (ack: StandardSocketSubscribeAck) => void;
    }) {
      const {
        lineage,
        onEvent,
        onFailure,
        onSubscribed,
      } = options;
      const resumeFromSeq = options.resumeFromSeq ?? lineage.last_seq;
      const resolveResumeFromSeq = () =>
        typeof resumeFromSeq === 'function'
          ? resumeFromSeq()
          : resumeFromSeq;
      const request = buildStandardSubscribeRequest(lineage, resolveResumeFromSeq());
      const subscriptionId = ++nextSubscriptionId;
      const subscription: StandardSocketSubscription<TPayload> = {
        id: subscriptionId,
        lineage,
        request,
        issueId: 0,
        resolveResumeFromSeq,
        onEvent,
        onFailure,
        onSubscribed,
      };

      subscriptions.set(subscriptionId, subscription);
      ensureSocketHandlers();
      if ((boundSocket ?? resolveSocket()).connected) {
        issueSubscribe(subscription);
      }

      return () => {
        const socketTarget = boundSocket;
        const removed = removeSubscription(subscriptionId);
        if (!removed) {
          return;
        }
        emitSurfaceUnsubscribeIfUnused(removed.request.surface, socketTarget);
      };
    },
    refreshSocketBinding() {
      const didRebind = refreshSocketBinding();
      if (!didRebind || !boundSocket?.connected) {
        return;
      }
      for (const subscription of subscriptions.values()) {
        issueSubscribe(subscription);
      }
    },
    resetForTests() {
      subscriptions.clear();
      detachSocketHandlers();
      nextSubscriptionId = 0;
    },
  };
}

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

const getFluxboardRuntimeConfig = (): FluxboardRuntimeConfig | null => {
  if (typeof window === 'undefined') {
    return null;
  }
  const runtimeConfig = (window as any).__FLUXBOARD_RUNTIME_CONFIG__;
  return runtimeConfig && typeof runtimeConfig === 'object'
    ? runtimeConfig as FluxboardRuntimeConfig
    : null;
};

const getSocketPathOverride = (profile: PathProfile): string | null => {
  const configuredPath = getFluxboardRuntimeConfig()?.socketPaths?.[profile];
  if (typeof configuredPath !== 'string') {
    return null;
  }
  const trimmed = configuredPath.trim();
  return trimmed || null;
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
    const socketPath = getSocketPathOverride(currentSocketProfile as PathProfile) ?? DEFAULT_SOCKET_PATH;
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
          path: socketPath,
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
  standardSocketClient.refreshSocketBinding();

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

export function subscribeToStandardRealtimeEvents(
  handler: (event: StandardRealtimeEvent) => void,
): () => void {
  socket.on('realtime_event', handler);
  return () => {
    socket.off('realtime_event', handler);
  };
}

export function emitStandardSubscribe(
  payload: StandardRealtimeSubscribePayload,
): Promise<StandardRealtimeSubscribeAck> {
  return new Promise((resolve) => {
    socket.emit('subscribe', payload, (ack: StandardRealtimeSubscribeAck) => {
      resolve(ack);
    });
  });
}

export function emitStandardUnsubscribe(
  payload: { surface: string },
): Promise<StandardRealtimeUnsubscribeAck> {
  return new Promise((resolve) => {
    socket.emit('unsubscribe', payload, (ack: StandardRealtimeUnsubscribeAck) => {
      resolve(ack);
    });
  });
}

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

export const standardSocketClient = createStandardSocketClient(
  () => getSocketExport() as unknown as StandardSocketLike,
);

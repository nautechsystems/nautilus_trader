// useWebSocket hook - Simplify Socket.IO subscriptions

import { useEffect, useRef, useSyncExternalStore } from 'react';
import {
  socket,
  standardSocketClient,
  type StandardSocketEventEnvelope,
  type StandardSocketFailure,
  type StandardSocketSubscribeAck,
} from '../sockets';
import { isRealtimeStandardEnabled } from '../config/featureFlags';
import {
  REALTIME_STANDARD_SURFACES,
  type RealtimeSurface,
} from '../lib/realtime/constants';
import type { RealtimeSnapshotLineage } from '../types';

export type WebSocketSubscriptionMode = 'legacy' | 'standard';

export type WebSocketSubscription<T = unknown> = (
  event: string,
  handler: (data: T) => void
) => () => void;

export type WebSocketBridge<T = unknown> = {
  resolveMode?: (options: {
    event: string;
    surface?: string;
  }) => WebSocketSubscriptionMode;
  subscribe: (options: {
    event: string;
    surface?: string;
    legacySubscribe: WebSocketSubscription<T>;
    handler: (data: T) => void;
  }) => () => void;
};

export type UseWebSocketOptions<T = unknown> = {
  surface?: string;
  subscribe?: WebSocketSubscription<T>;
  bridge?: WebSocketBridge<T>;
};

let sharedWebSocketBridge: WebSocketBridge<any> | null = null;
const sharedWebSocketBridgeListeners = new Set<() => void>();

function notifySharedWebSocketBridgeListeners(): void {
  for (const listener of sharedWebSocketBridgeListeners) {
    listener();
  }
}

function subscribeToSharedWebSocketBridge(listener: () => void): () => void {
  sharedWebSocketBridgeListeners.add(listener);
  return () => {
    sharedWebSocketBridgeListeners.delete(listener);
  };
}

function getSharedWebSocketBridgeSnapshot(): WebSocketBridge<any> | null {
  return sharedWebSocketBridge;
}

function subscribeToNoopSharedBridge(): () => void {
  return () => {};
}

function getNoSharedWebSocketBridgeSnapshot(): null {
  return null;
}

export function registerSharedWebSocketBridge<T = unknown>(
  bridge: WebSocketBridge<T>,
): void {
  sharedWebSocketBridge = bridge as WebSocketBridge<any>;
  notifySharedWebSocketBridgeListeners();
}

export function resetSharedWebSocketBridgeForTests(): void {
  sharedWebSocketBridge = null;
  notifySharedWebSocketBridgeListeners();
}

export type UseStandardWebSocketSubscriptionOptions<TPayload = unknown> = {
  enabled?: boolean;
  lineage?: RealtimeSnapshotLineage | null;
  resumeFromSeq?: number | (() => number);
  onEvent: (event: StandardSocketEventEnvelope<TPayload>) => void;
  onFailure?: (failure: StandardSocketFailure) => void;
  onSubscribed?: (ack: StandardSocketSubscribeAck) => void;
};

function subscribeToSocket<T = unknown>(
  event: string,
  handler: (data: T) => void,
): () => void {
  socket.on(event, handler);
  return () => {
    socket.off(event, handler);
  };
}

export function useStandardWebSocketSubscription<TPayload = unknown>({
  enabled = true,
  lineage,
  resumeFromSeq,
  onEvent,
  onFailure,
  onSubscribed,
}: UseStandardWebSocketSubscriptionOptions<TPayload>): void {
  const lineageKey = enabled && lineage
    ? [
        lineage.contract_version,
        lineage.surface,
        lineage.profile,
        lineage.surface_query_key,
        lineage.stream_id,
        String(lineage.snapshot_revision),
      ].join('|')
    : 'disabled';
  const onEventRef = useRef(onEvent);
  const onFailureRef = useRef(onFailure);
  const onSubscribedRef = useRef(onSubscribed);
  const lineageRef = useRef<RealtimeSnapshotLineage | null>(lineage ?? null);
  const resumeFromSeqRef = useRef<number | (() => number) | undefined>(resumeFromSeq);

  useEffect(() => {
    onEventRef.current = onEvent;
  }, [onEvent]);

  useEffect(() => {
    onFailureRef.current = onFailure;
  }, [onFailure]);

  useEffect(() => {
    onSubscribedRef.current = onSubscribed;
  }, [onSubscribed]);

  useEffect(() => {
    lineageRef.current = lineage ?? null;
  }, [lineageKey, lineage]);

  useEffect(() => {
    resumeFromSeqRef.current = resumeFromSeq;
  }, [resumeFromSeq]);

  useEffect(() => {
    if (!enabled || !lineage) {
      return undefined;
    }

    return standardSocketClient.subscribe<TPayload>({
      lineage,
      resumeFromSeq: () => {
        const currentLineage = lineageRef.current ?? lineage;
        const resolved = typeof resumeFromSeqRef.current === 'function'
          ? resumeFromSeqRef.current()
          : resumeFromSeqRef.current;
        if (typeof resolved === 'number' && Number.isFinite(resolved)) {
          return resolved;
        }
        return currentLineage.last_seq;
      },
      onEvent: (event) => {
        onEventRef.current(event as StandardSocketEventEnvelope<TPayload>);
      },
      onFailure: (failure) => {
        onFailureRef.current?.(failure as StandardSocketFailure);
      },
      onSubscribed: (ack) => {
        onSubscribedRef.current?.(ack as StandardSocketSubscribeAck);
      },
    });
  }, [enabled, lineageKey, lineage]);
}

function isRealtimeSurface(surface?: string): surface is RealtimeSurface {
  return surface !== undefined
    && (REALTIME_STANDARD_SURFACES as readonly string[]).includes(surface);
}

/**
 * Hook to subscribe to WebSocket events with automatic cleanup
 *
 * @param event - WebSocket event name to subscribe to
 * @param handler - Event handler function
 * @param options - Optional legacy/standard bridge configuration
 *
 * @example
 * ```tsx
 * useWebSocket('market_update', (payload) => {
 *   setMarketData(payload.market_data);
 * });
 * ```
 */
export function useWebSocket<T = unknown>(
  event: string,
  handler: (data: T) => void,
  options?: UseWebSocketOptions<T>,
): void {
  // Use ref to always have the latest handler without recreating the subscription
  const handlerRef = useRef(handler);
  const surface = options?.surface;
  const surfaceUsesRealtimeStandard = isRealtimeSurface(surface) && isRealtimeStandardEnabled(surface);
  const usesSharedBridgeStore = options?.bridge === undefined && surfaceUsesRealtimeStandard;
  const registeredSharedBridge = useSyncExternalStore(
    usesSharedBridgeStore ? subscribeToSharedWebSocketBridge : subscribeToNoopSharedBridge,
    usesSharedBridgeStore ? getSharedWebSocketBridgeSnapshot : getNoSharedWebSocketBridgeSnapshot,
    usesSharedBridgeStore ? getSharedWebSocketBridgeSnapshot : getNoSharedWebSocketBridgeSnapshot,
  );
  const legacySubscribe: WebSocketSubscription<T> = options?.subscribe ?? subscribeToSocket;
  const activeBridge = (options?.bridge ?? registeredSharedBridge) as WebSocketBridge<T> | null;
  const bridgeSubscribe = activeBridge?.subscribe;
  const explicitMode = activeBridge?.resolveMode?.({ event, surface });
  const mode = explicitMode ?? (
    bridgeSubscribe && surfaceUsesRealtimeStandard
      ? 'standard'
      : 'legacy'
  );
  const activeBridgeSubscribe = mode === 'standard' ? bridgeSubscribe ?? null : null;

  useEffect(() => {
    handlerRef.current = handler;
  }, [handler]);

  useEffect(() => {
    const wrappedLegacyHandler = (data: T) => {
      handlerRef.current(data);
    };

    if (activeBridgeSubscribe) {
      return activeBridgeSubscribe({
        event,
        surface,
        legacySubscribe,
        handler: (data: T) => {
          handlerRef.current(data);
        },
      });
    }

    return legacySubscribe(event, wrappedLegacyHandler);
  }, [activeBridgeSubscribe, event, legacySubscribe, surface]);
}

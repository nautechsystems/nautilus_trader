// useWebSocket hook - Simplify Socket.IO subscriptions

import { useEffect, useRef, useSyncExternalStore } from 'react';
import { socket } from '../sockets';
import { isRealtimeStandardEnabled } from '../config/featureFlags';
import {
  REALTIME_STANDARD_SURFACES,
  type RealtimeSurface,
} from '../lib/realtime/constants';

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

function subscribeToSocket<T = unknown>(
  event: string,
  handler: (data: T) => void,
): () => void {
  socket.on(event, handler);
  return () => {
    socket.off(event, handler);
  };
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

  useEffect(() => {
    handlerRef.current = handler;
  }, [handler]);

  useEffect(() => {
    const wrappedLegacyHandler = (data: T) => {
      handlerRef.current(data);
    };

    if (mode === 'standard' && bridgeSubscribe) {
      return bridgeSubscribe({
        event,
        surface,
        legacySubscribe,
        handler: (data: T) => {
          handlerRef.current(data);
        },
      });
    }

    return legacySubscribe(event, wrappedLegacyHandler);
  }, [bridgeSubscribe, event, legacySubscribe, mode, surface]);
}

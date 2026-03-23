// useWebSocket hook - Simplify Socket.IO subscriptions

import { useEffect, useRef } from 'react';
import { socket } from '../sockets';

export type WebSocketSubscriptionMode = 'legacy' | 'standard';

export type WebSocketSubscription<T = unknown> = (
  event: string,
  handler: (data: T) => void
) => () => void;

export type WebSocketBridge<TLegacy = unknown, TStandard = TLegacy> = {
  resolveMode?: (options: {
    event: string;
    surface?: string;
  }) => WebSocketSubscriptionMode;
  subscribe: (options: {
    event: string;
    surface?: string;
    legacySubscribe: WebSocketSubscription<TLegacy>;
    handler: (data: TStandard) => void;
  }) => () => void;
};

export type UseWebSocketOptions<TLegacy = unknown, TStandard = TLegacy> = {
  surface?: string;
  subscribe?: WebSocketSubscription<TLegacy>;
  bridge?: WebSocketBridge<TLegacy, TStandard>;
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
export function useWebSocket<TLegacy = unknown, TStandard = TLegacy>(
  event: string,
  handler: (data: TStandard) => void,
  options?: UseWebSocketOptions<TLegacy, TStandard>,
): void {
  // Use ref to always have the latest handler without recreating the subscription
  const handlerRef = useRef(handler);
  const surface = options?.surface;
  const legacySubscribe: WebSocketSubscription<TLegacy> = options?.subscribe ?? subscribeToSocket;
  const bridgeSubscribe = options?.bridge?.subscribe;
  const mode = bridgeSubscribe
    ? options?.bridge?.resolveMode?.({ event, surface }) ?? 'legacy'
    : 'legacy';

  useEffect(() => {
    handlerRef.current = handler;
  }, [handler]);

  useEffect(() => {
    const wrappedLegacyHandler = (data: TLegacy) => {
      handlerRef.current(data as TStandard);
    };

    if (mode === 'standard' && bridgeSubscribe) {
      return bridgeSubscribe({
        event,
        surface,
        legacySubscribe,
        handler: (data: TStandard) => {
          handlerRef.current(data);
        },
      });
    }

    return legacySubscribe(event, wrappedLegacyHandler);
  }, [bridgeSubscribe, event, legacySubscribe, mode, surface]);
}

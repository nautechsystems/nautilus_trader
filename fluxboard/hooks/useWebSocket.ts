// useWebSocket hook - Simplify Socket.IO subscriptions

import { useEffect, useRef } from 'react';
import { socket } from '../sockets';

/**
 * Hook to subscribe to WebSocket events with automatic cleanup
 *
 * @param event - WebSocket event name to subscribe to
 * @param handler - Event handler function
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
  handler: (data: T) => void
): void {
  // Use ref to always have the latest handler without recreating the subscription
  const handlerRef = useRef(handler);

  useEffect(() => {
    handlerRef.current = handler;
  }, [handler]);

  useEffect(() => {
    const wrappedHandler = (data: T) => {
      handlerRef.current(data);
    };

    socket.on(event, wrappedHandler);

    return () => {
      socket.off(event, wrappedHandler);
    };
  }, [event]);
}

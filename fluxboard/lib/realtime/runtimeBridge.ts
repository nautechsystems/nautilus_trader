import { isRealtimeStandardEnabled } from '../../config/featureFlags';
import {
  registerSharedWebSocketBridge,
  type WebSocketBridge,
} from '../../hooks/useWebSocket';
import {
  REALTIME_STANDARD_SURFACES,
  type RealtimeSurface,
} from './constants';

function isRealtimeSurface(surface?: string): surface is RealtimeSurface {
  return surface !== undefined
    && (REALTIME_STANDARD_SURFACES as readonly string[]).includes(surface);
}

export const sharedRealtimeWebSocketBridge: WebSocketBridge<unknown> = {
  resolveMode: ({ surface }) => (
    isRealtimeSurface(surface) && isRealtimeStandardEnabled(surface)
      ? 'standard'
      : 'legacy'
  ),
  subscribe: ({ event, legacySubscribe, handler }) => legacySubscribe(event, handler),
};

registerSharedWebSocketBridge(sharedRealtimeWebSocketBridge);

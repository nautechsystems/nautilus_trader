import { useEffect, useMemo, useRef, useState } from 'react';
import {
  createRecoveryScheduler,
  type RecoveryEvent,
} from './useRecoveryScheduler';
import type {
  RealtimeRowDelta,
  RealtimeSurfaceController,
} from './useRealtimeSurfaceController';

export type RealtimeChannelHandlers<TSnapshot, TDelta> = {
  onOpen?: () => void;
  onSnapshot?: (snapshot: TSnapshot) => void;
  onDelta?: (delta: TDelta) => void;
  onClose?: (reason?: unknown) => void;
  onError?: (error: unknown) => void;
};

export interface RealtimeChannelAdapter<TSnapshot, TDelta> {
  connect: (handlers: RealtimeChannelHandlers<TSnapshot, TDelta>) => (() => void) | void;
}

export interface RealtimeChannelState {
  status: 'idle' | 'connecting' | 'live' | 'recovering';
  reconnectAttempt: number;
  lastEventAt?: number;
  lastCloseReason?: string;
}

export interface UseRealtimeChannelOptions<TRow> {
  channelKey: string;
  enabled?: boolean;
  adapter: RealtimeChannelAdapter<readonly TRow[], RealtimeRowDelta<TRow> | readonly RealtimeRowDelta<TRow>[]>;
  controller: Pick<RealtimeSurfaceController<TRow>, 'applySnapshot' | 'queueDelta' | 'flush'>;
  recoveryBaseDelayMs?: number;
  recoveryMaxDelayMs?: number;
  onRecover?: (event: RecoveryEvent) => void;
}

export { createRealtimeSurfaceController } from './useRealtimeSurfaceController';

export function useRealtimeChannel<TRow>({
  channelKey,
  enabled = true,
  adapter,
  controller,
  recoveryBaseDelayMs = 1_000,
  recoveryMaxDelayMs = 30_000,
  onRecover,
}: UseRealtimeChannelOptions<TRow>): RealtimeChannelState {
  const [reconnectToken, setReconnectToken] = useState(0);
  const [state, setState] = useState<RealtimeChannelState>({
    status: enabled ? 'connecting' : 'idle',
    reconnectAttempt: 0,
  });

  const controllerRef = useRef(controller);
  controllerRef.current = controller;

  const scheduler = useMemo(
    () =>
      createRecoveryScheduler({
        baseDelayMs: recoveryBaseDelayMs,
        maxDelayMs: recoveryMaxDelayMs,
        onRecover: (event) => {
          onRecover?.(event);
          setReconnectToken((value) => value + 1);
        },
      }),
    [channelKey, recoveryBaseDelayMs, recoveryMaxDelayMs, onRecover],
  );

  useEffect(() => {
    return () => {
      scheduler.dispose();
    };
  }, [scheduler]);

  useEffect(() => {
    if (!enabled) {
      setState({
        status: 'idle',
        reconnectAttempt: 0,
      });
      scheduler.cancel();
      return undefined;
    }

    setState((previous) => ({
      ...previous,
      status: 'connecting',
    }));

    const disconnect = adapter.connect({
      onOpen: () => {
        scheduler.reset();
        setState((previous) => ({
          ...previous,
          status: 'live',
          reconnectAttempt: 0,
        }));
      },
      onSnapshot: (snapshot) => {
        controllerRef.current.applySnapshot(snapshot);
        setState((previous) => ({
          ...previous,
          lastEventAt: Date.now(),
        }));
      },
      onDelta: (delta) => {
        controllerRef.current.queueDelta(delta);
        setState((previous) => ({
          ...previous,
          lastEventAt: Date.now(),
        }));
      },
      onClose: (reason) => {
        const reasonText = String(reason ?? 'closed');
        const delayMs = scheduler.schedule(reasonText);
        const nextSnapshot = scheduler.getSnapshot();
        setState((previous) => ({
          ...previous,
          status: 'recovering',
          reconnectAttempt: nextSnapshot.pending ? nextSnapshot.attempt : previous.reconnectAttempt,
          lastCloseReason: reasonText,
          lastEventAt: delayMs ? previous.lastEventAt : previous.lastEventAt,
        }));
      },
      onError: (error) => {
        const reasonText = String(error ?? 'error');
        scheduler.schedule(reasonText);
        const nextSnapshot = scheduler.getSnapshot();
        setState((previous) => ({
          ...previous,
          status: 'recovering',
          reconnectAttempt: nextSnapshot.attempt,
          lastCloseReason: reasonText,
        }));
      },
    });

    return () => {
      if (typeof disconnect === 'function') {
        disconnect();
      }
    };
  }, [adapter, controllerRef, enabled, reconnectToken, scheduler]);

  return state;
}

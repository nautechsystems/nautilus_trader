import { useEffect, useMemo, useRef, useState } from 'react';
import {
  socket,
  type StandardRealtimeEvent,
  type StandardRealtimeSubscribeAck,
  standardSocketClient,
  type StandardSocketFailure,
} from '../sockets';
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
  status: 'idle' | 'connecting' | 'live' | 'recovering' | 'manual_refresh_required';
  reconnectAttempt: number;
  lastEventAt?: number;
  lastCloseReason?: string;
}

export interface UseRealtimeChannelOptions<TRow> {
  channelKey: string;
  enabled?: boolean;
  adapter: RealtimeChannelAdapter<readonly TRow[], RealtimeRowDelta<TRow> | readonly RealtimeRowDelta<TRow>[]>;
  controller: Pick<RealtimeSurfaceController<TRow>, 'applySnapshot' | 'queueDelta' | 'flush' | 'clearQueuedDeltas'>;
  recoveryBaseDelayMs?: number;
  recoveryMaxDelayMs?: number;
  onRecover?: (event: RecoveryEvent) => void;
}

export { createRealtimeSurfaceController } from './useRealtimeSurfaceController';

export type StandardRealtimeLineage = {
  contractVersion: number;
  surface: string;
  profile: string;
  surfaceQueryKey: string;
  streamId: string;
  snapshotRevision: number | string;
  resumeFromSeq: number | (() => number);
};

export interface UseStandardRealtimeSubscriptionOptions {
  enabled?: boolean;
  lineage?: StandardRealtimeLineage | null;
  onSubscribed?: (ack: StandardRealtimeSubscribeAck) => void;
  onRejected?: (ack: StandardRealtimeSubscribeAck) => void;
  onEvent?: (event: StandardRealtimeEvent) => void;
  onConnect?: () => void;
  onDisconnect?: (reason?: unknown) => void;
  onConnectError?: (error: unknown) => void;
  onReconnectAttempt?: (attempt: number) => void;
}

export function useStandardRealtimeSubscription({
  enabled = true,
  lineage,
  onSubscribed,
  onRejected,
  onEvent,
  onConnect,
  onDisconnect,
  onConnectError,
  onReconnectAttempt,
}: UseStandardRealtimeSubscriptionOptions): void {
  const lineageRef = useRef<StandardRealtimeLineage | null>(lineage ?? null);
  const lineageKey = enabled && lineage
    ? [
        lineage.contractVersion,
        lineage.surface,
        lineage.profile,
        lineage.surfaceQueryKey,
        lineage.streamId,
        String(lineage.snapshotRevision),
      ].join('|')
    : 'disabled';

  useEffect(() => {
    lineageRef.current = lineage ?? null;
  }, [lineageKey, lineage]);

  useEffect(() => {
    let isActive = true;
    const current = enabled ? lineageRef.current : null;
    const unsubscribeStandard = current
      ? standardSocketClient.subscribe({
          lineage: {
            contract_version: current.contractVersion,
            surface: current.surface,
            profile: current.profile,
            surface_query_key: current.surfaceQueryKey,
            stream_id: current.streamId,
            snapshot_revision: current.snapshotRevision,
            last_seq:
              typeof current.resumeFromSeq === 'function'
                ? current.resumeFromSeq()
                : current.resumeFromSeq,
          },
          resumeFromSeq: current.resumeFromSeq,
          onEvent: (event) => {
            if (!isActive) {
              return;
            }
            onEvent?.(event as StandardRealtimeEvent);
          },
          onFailure: (failure: StandardSocketFailure) => {
            if (!isActive) {
              return;
            }
            const rejection = (
              failure.type === 'recovery_required'
                ? undefined
                : failure.ack
            ) ?? {
              accepted: false,
              contract_version: failure.requested.contract_version,
              surface: failure.requested.surface,
              profile: failure.requested.profile,
              surface_query_key: failure.requested.surface_query_key,
              stream_id: failure.requested.stream_id,
              snapshot_revision: failure.requested.snapshot_revision,
              reason: failure.reason,
            };
            onRejected?.(rejection as StandardRealtimeSubscribeAck);
          },
          onSubscribed: (ack) => {
            if (!isActive) {
              return;
            }
            onSubscribed?.(ack as StandardRealtimeSubscribeAck);
          },
        })
      : undefined;

    const handleConnect = () => {
      onConnect?.();
    };

    const handleDisconnect = (reason?: unknown) => {
      onDisconnect?.(reason);
    };

    const handleConnectError = (error: unknown) => {
      onConnectError?.(error);
    };

    const handleReconnectAttempt = (attempt: number) => {
      onReconnectAttempt?.(attempt);
    };

    socket.on('connect', handleConnect);
    socket.on('disconnect', handleDisconnect);
    socket.on('connect_error', handleConnectError);
    socket.on('reconnect_attempt', handleReconnectAttempt);

    if (enabled && socket.connected) {
      handleConnect();
    }

    return () => {
      isActive = false;
      socket.off('connect', handleConnect);
      socket.off('disconnect', handleDisconnect);
      socket.off('connect_error', handleConnectError);
      socket.off('reconnect_attempt', handleReconnectAttempt);
      unsubscribeStandard?.();
    };
  }, [
    enabled,
    lineageKey,
    onConnect,
    onConnectError,
    onDisconnect,
    onEvent,
    onReconnectAttempt,
    onRejected,
    onSubscribed,
  ]);
}

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
  const connectionIdRef = useRef(0);
  const activeConnectionIdRef = useRef(0);

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
      activeConnectionIdRef.current = 0;
      controllerRef.current.clearQueuedDeltas();
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

    const connectionId = connectionIdRef.current + 1;
    connectionIdRef.current = connectionId;
    activeConnectionIdRef.current = connectionId;
    const isActiveConnection = () => activeConnectionIdRef.current === connectionId;

    const disconnect = adapter.connect({
      onOpen: () => {
        if (!isActiveConnection()) {
          return;
        }
        scheduler.reset();
        setState((previous) => ({
          ...previous,
          status: 'live',
          reconnectAttempt: 0,
        }));
      },
      onSnapshot: (snapshot) => {
        if (!isActiveConnection()) {
          return;
        }
        controllerRef.current.clearQueuedDeltas();
        controllerRef.current.applySnapshot(snapshot);
        setState((previous) => ({
          ...previous,
          lastEventAt: Date.now(),
        }));
      },
      onDelta: (delta) => {
        if (!isActiveConnection()) {
          return;
        }
        controllerRef.current.queueDelta(delta);
        setState((previous) => ({
          ...previous,
          lastEventAt: Date.now(),
        }));
      },
      onClose: (reason) => {
        if (!isActiveConnection()) {
          return;
        }
        activeConnectionIdRef.current = 0;
        controllerRef.current.clearQueuedDeltas();
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
        if (!isActiveConnection()) {
          return;
        }
        activeConnectionIdRef.current = 0;
        controllerRef.current.clearQueuedDeltas();
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
      if (activeConnectionIdRef.current === connectionId) {
        activeConnectionIdRef.current = 0;
      }
      if (typeof disconnect === 'function') {
        disconnect();
      }
    };
  }, [adapter, controllerRef, enabled, reconnectToken, scheduler]);

  return state;
}

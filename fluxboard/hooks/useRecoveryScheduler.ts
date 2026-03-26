import { useEffect, useMemo, useRef, useState } from 'react';

export interface RecoveryEvent {
  attempt: number;
  delayMs: number;
  reason: string;
  scheduledAt: number;
  runAt: number;
}

export interface RecoverySchedulerSnapshot {
  attempt: number;
  pending: boolean;
  nextDelayMs?: number;
  nextRunAt?: number;
  lastReason?: string;
}

export interface RecoverySchedulerOptions {
  baseDelayMs?: number;
  maxDelayMs?: number;
  multiplier?: number;
  onRecover?: (event: RecoveryEvent) => void;
}

export interface RecoveryScheduler {
  getSnapshot: () => RecoverySchedulerSnapshot;
  subscribe: (listener: () => void) => () => void;
  schedule: (reason?: string) => number;
  cancel: () => void;
  reset: () => void;
  dispose: () => void;
}

function isScheduler(value: unknown): value is RecoveryScheduler {
  return Boolean(value && typeof value === 'object' && typeof (value as RecoveryScheduler).schedule === 'function');
}

export function createRecoveryScheduler({
  baseDelayMs = 1_000,
  maxDelayMs = 30_000,
  multiplier = 2,
  onRecover,
}: RecoverySchedulerOptions = {}): RecoveryScheduler {
  let snapshot: RecoverySchedulerSnapshot = {
    attempt: 0,
    pending: false,
  };
  let timerId: ReturnType<typeof setTimeout> | null = null;
  const listeners = new Set<() => void>();

  const notify = () => {
    listeners.forEach((listener) => listener());
  };

  const clearTimer = () => {
    if (timerId !== null) {
      clearTimeout(timerId);
      timerId = null;
    }
  };

  return {
    getSnapshot: () => snapshot,
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    schedule(reason = 'retry') {
      if (snapshot.pending && snapshot.nextDelayMs !== undefined) {
        return snapshot.nextDelayMs;
      }

      const delayMs = Math.min(maxDelayMs, Math.round(baseDelayMs * multiplier ** snapshot.attempt));
      const scheduledAt = Date.now();
      const runAt = scheduledAt + delayMs;
      snapshot = {
        ...snapshot,
        pending: true,
        nextDelayMs: delayMs,
        nextRunAt: runAt,
        lastReason: reason,
      };
      notify();

      timerId = setTimeout(() => {
        timerId = null;
        const nextAttempt = snapshot.attempt + 1;
        snapshot = {
          attempt: nextAttempt,
          pending: false,
          nextDelayMs: undefined,
          nextRunAt: undefined,
          lastReason: reason,
        };
        notify();
        onRecover?.({
          attempt: nextAttempt,
          delayMs,
          reason,
          scheduledAt,
          runAt,
        });
      }, delayMs);

      return delayMs;
    },
    cancel() {
      clearTimer();
      snapshot = {
        ...snapshot,
        pending: false,
        nextDelayMs: undefined,
        nextRunAt: undefined,
      };
      notify();
    },
    reset() {
      clearTimer();
      snapshot = {
        attempt: 0,
        pending: false,
        nextDelayMs: undefined,
        nextRunAt: undefined,
        lastReason: snapshot.lastReason,
      };
      notify();
    },
    dispose() {
      clearTimer();
      listeners.clear();
    },
  };
}

export function useRecoveryScheduler(options: RecoverySchedulerOptions | RecoveryScheduler = {}) {
  const ownsScheduler = !isScheduler(options);
  const scheduler = useMemo(
    () =>
      ownsScheduler
        ? createRecoveryScheduler(options as RecoverySchedulerOptions)
        : options,
    [
      ownsScheduler ? (options as RecoverySchedulerOptions).baseDelayMs : options,
      ownsScheduler ? (options as RecoverySchedulerOptions).maxDelayMs : options,
      ownsScheduler ? (options as RecoverySchedulerOptions).multiplier : options,
      ownsScheduler ? (options as RecoverySchedulerOptions).onRecover : options,
    ],
  );

  const schedulerRef = useRef(scheduler);
  schedulerRef.current = scheduler;
  const [snapshot, setSnapshot] = useState<RecoverySchedulerSnapshot>(() => scheduler.getSnapshot());

  useEffect(() => {
    setSnapshot(scheduler.getSnapshot());
    return scheduler.subscribe(() => {
      setSnapshot(scheduler.getSnapshot());
    });
  }, [scheduler]);

  useEffect(() => {
    if (!ownsScheduler) {
      return undefined;
    }
    return () => {
      scheduler.dispose();
    };
  }, [ownsScheduler, scheduler]);

  return {
    ...snapshot,
    schedule: (reason?: string) => schedulerRef.current.schedule(reason),
    cancel: () => schedulerRef.current.cancel(),
    reset: () => schedulerRef.current.reset(),
    scheduler,
  };
}

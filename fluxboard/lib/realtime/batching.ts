export type BatchCancel = () => void;
export type BatchSchedule = (flush: () => void) => BatchCancel | void;

export interface PendingBatcher<T> {
  enqueue: (value: T | readonly T[]) => void;
  flush: () => readonly T[];
  cancel: () => void;
  size: () => number;
  pending: () => boolean;
}

function defaultBatchSchedule(flush: () => void): BatchCancel {
  const id = setTimeout(flush, 0);
  return () => clearTimeout(id);
}

export function createPendingBatcher<T>({
  onFlush,
  schedule = defaultBatchSchedule,
}: {
  onFlush: (items: readonly T[]) => void;
  schedule?: BatchSchedule;
}): PendingBatcher<T> {
  let queue: T[] = [];
  let cancelScheduled: BatchCancel | null = null;

  const flushNow = (): readonly T[] => {
    if (cancelScheduled) {
      cancelScheduled();
      cancelScheduled = null;
    }
    if (queue.length === 0) {
      return [];
    }
    const batch = queue;
    queue = [];
    onFlush(batch);
    return batch;
  };

  return {
    enqueue(value) {
      if (Array.isArray(value)) {
        queue.push(...(value as readonly T[]));
      } else {
        queue.push(value as T);
      }

      if (!cancelScheduled) {
        cancelScheduled = schedule(flushNow) ?? null;
      }
    },
    flush() {
      return flushNow();
    },
    cancel() {
      if (cancelScheduled) {
        cancelScheduled();
        cancelScheduled = null;
      }
      queue = [];
    },
    size() {
      return queue.length;
    },
    pending() {
      return queue.length > 0;
    },
  };
}

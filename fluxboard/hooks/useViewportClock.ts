import { useEffect, useId, useState } from 'react';

type ViewportClockSubscriber = {
  active: boolean;
  intervalMs: number;
  now: number;
  remainingMs: number;
  listeners: Set<() => void>;
};

type ViewportClockStore = {
  key: string;
  subscribers: Map<string, ViewportClockSubscriber>;
  timerId: ReturnType<typeof setInterval> | null;
  tickCount: number;
};

export interface ViewportClockDebugState {
  activeSubscriberCount: number;
  timerCount: number;
  tickCount: number;
  subscriberIds: string[];
}

export interface UseViewportClockOptions {
  clockKey?: string;
  subscriberId?: string;
  intervalMs?: number;
  active?: boolean;
}

const DEFAULT_CLOCK_KEY = 'fluxboard:viewport-clock';
const clockRegistry = new Map<string, ViewportClockStore>();

function getOrCreateClockStore(clockKey: string): ViewportClockStore {
  const existing = clockRegistry.get(clockKey);
  if (existing) {
    return existing;
  }

  const store: ViewportClockStore = {
    key: clockKey,
    subscribers: new Map(),
    timerId: null,
    tickCount: 0,
  };
  clockRegistry.set(clockKey, store);
  return store;
}

function getActiveSubscribers(store: ViewportClockStore) {
  return Array.from(store.subscribers.values()).filter((subscriber) => subscriber.active);
}

function syncClockTimer(store: ViewportClockStore) {
  const activeSubscribers = getActiveSubscribers(store);
  const nextInterval = activeSubscribers.reduce((current, subscriber) => {
    if (current === null || subscriber.intervalMs < current) {
      return subscriber.intervalMs;
    }
    return current;
  }, null as number | null);

  if (nextInterval === null) {
    if (store.timerId !== null) {
      clearInterval(store.timerId);
      store.timerId = null;
    }
    return;
  }

  if (store.timerId !== null) {
    clearInterval(store.timerId);
    store.timerId = null;
  }

  store.timerId = setInterval(() => {
    const nextNow = Date.now();
    store.tickCount += 1;
    store.subscribers.forEach((subscriber) => {
      if (!subscriber.active) {
        return;
      }
      subscriber.remainingMs -= nextInterval;
      if (subscriber.remainingMs > 0) {
        return;
      }
      subscriber.now = nextNow;
      subscriber.remainingMs = subscriber.intervalMs;
      subscriber.listeners.forEach((listener) => listener());
    });
  }, nextInterval);
}

function subscribeToClock({
  clockKey,
  subscriberId,
  intervalMs,
  active,
  onTick,
}: {
  clockKey: string;
  subscriberId: string;
  intervalMs: number;
  active: boolean;
  onTick: () => void;
}) {
  const store = getOrCreateClockStore(clockKey);
  const subscriber = store.subscribers.get(subscriberId) ?? {
    active,
    intervalMs,
    now: Date.now(),
    remainingMs: intervalMs,
    listeners: new Set<() => void>(),
  };

  subscriber.active = active;
  subscriber.intervalMs = intervalMs;
  subscriber.remainingMs = intervalMs;
  subscriber.listeners.add(onTick);
  if (!store.subscribers.has(subscriberId)) {
    store.subscribers.set(subscriberId, subscriber);
  }

  syncClockTimer(store);

  return () => {
    const current = store.subscribers.get(subscriberId);
    if (!current) {
      return;
    }
    current.listeners.delete(onTick);
    if (current.listeners.size === 0) {
      store.subscribers.delete(subscriberId);
    }
    syncClockTimer(store);
  };
}

export function __resetViewportClockRegistryForTests() {
  clockRegistry.forEach((store) => {
    if (store.timerId !== null) {
      clearInterval(store.timerId);
    }
  });
  clockRegistry.clear();
}

export function getViewportClockDebugState(clockKey: string): ViewportClockDebugState | undefined {
  const store = clockRegistry.get(clockKey);
  if (!store) {
    return undefined;
  }
  return {
    activeSubscriberCount: getActiveSubscribers(store).length,
    timerCount: store.timerId ? 1 : 0,
    tickCount: store.tickCount,
    subscriberIds: Array.from(store.subscribers.keys()),
  };
}

export function useViewportClock({
  clockKey = DEFAULT_CLOCK_KEY,
  subscriberId,
  intervalMs = 1_000,
  active = true,
}: UseViewportClockOptions = {}) {
  const autoId = useId();
  const resolvedSubscriberId = subscriberId ?? autoId;
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    const store = getOrCreateClockStore(clockKey);
    const existing = store.subscribers.get(resolvedSubscriberId);
    setNow(existing?.now ?? Date.now());

    return subscribeToClock({
      clockKey,
      subscriberId: resolvedSubscriberId,
      intervalMs,
      active,
      onTick: () => {
        const latest = getOrCreateClockStore(clockKey).subscribers.get(resolvedSubscriberId)?.now ?? Date.now();
        setNow(latest);
      },
    });
  }, [clockKey, resolvedSubscriberId, intervalMs, active]);

  return now;
}

import { act, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import SignalTable from './SignalTable';
import * as apiModule from '../../../api';
import { socket as mockSocket } from '../../../sockets';
import { useSignalStore } from '../../../stores';

vi.mock('../../../api', () => ({
  api: {
    getSignalStrategies: vi.fn(),
  },
}));

vi.mock('../../../sockets', () => {
  const handlers: Record<string, Array<(payload: any) => void>> = {};
  const socket = {
    connected: true,
    on: vi.fn((event: string, handler: (payload: any) => void) => {
      if (!handlers[event]) handlers[event] = [];
      handlers[event].push(handler);
    }),
    off: vi.fn((event: string, handler: (payload: any) => void) => {
      if (!handlers[event]) return;
      handlers[event] = handlers[event].filter((h) => h !== handler);
    }),
    emit: (event: string, payload?: any) => {
      (handlers[event] || []).forEach((h) => h(payload));
    },
  };
  (socket as any).__handlers = handlers;
  return { socket };
});

const originalIntersectionObserver = globalThis.IntersectionObserver;

class MockIntersectionObserver implements IntersectionObserver {
  readonly root: Element | Document | null = null;
  readonly rootMargin = '0px';
  readonly thresholds = [0];
  private readonly callback: IntersectionObserverCallback;
  private readonly elements = new Set<Element>();

  constructor(callback: IntersectionObserverCallback) {
    this.callback = callback;
    observerRegistry.push(this);
  }

  disconnect(): void {
    this.elements.clear();
  }

  observe(target: Element): void {
    this.elements.add(target);
  }

  takeRecords(): IntersectionObserverEntry[] {
    return [];
  }

  unobserve(target: Element): void {
    this.elements.delete(target);
  }

  emit(isIntersecting: boolean): void {
    const entries = Array.from(this.elements).map((target) => ({
      target,
      isIntersecting,
      intersectionRatio: isIntersecting ? 1 : 0,
      time: Date.now(),
      boundingClientRect: target.getBoundingClientRect(),
      intersectionRect: target.getBoundingClientRect(),
      rootBounds: null,
    })) as IntersectionObserverEntry[];

    this.callback(entries, this);
  }
}

const observerRegistry: MockIntersectionObserver[] = [];

function emitVisibility(isIntersecting: boolean) {
  observerRegistry.forEach((observer) => {
    observer.emit(isIntersecting);
  });
}

function createStrategy(serverTsMs: number) {
  return {
    id: 'visible_age',
    params: { bot_on: '1' },
    legs: {
      A: {
        coin: 'PLUME',
        exchange: 'bybit',
        md_ts_ms: serverTsMs - 10_000,
        update_time: '2025-01-15 12:00:00',
      },
      B: {
        coin: 'pUSD',
        exchange: 'rooster',
        md_ts_ms: serverTsMs - 500,
        update_time: '2025-01-15 12:00:09',
      },
    },
    balances_ok: true,
  } as any;
}

function createAgedStrategy({
  id,
  ageMs,
  serverTsMs,
}: {
  id: string;
  ageMs: number;
  serverTsMs: number;
}) {
  return {
    id,
    params: { bot_on: '1' },
    legs: {
      A: {
        coin: 'PLUME',
        exchange: 'bybit',
        md_ts_ms: serverTsMs - ageMs,
      },
      B: {
        coin: 'pUSD',
        exchange: 'rooster',
        md_ts_ms: serverTsMs - ageMs,
      },
    },
    balances_ok: true,
  } as any;
}

function createMissingAgeStrategy(id: string) {
  return {
    id,
    params: { bot_on: '1' },
    legs: {
      A: { coin: 'PLUME', exchange: 'bybit' },
      B: { coin: 'pUSD', exchange: 'rooster' },
    },
    balances_ok: true,
  } as any;
}

function getAgeText(container: HTMLElement): string {
  const ageCell = container.querySelector('tbody tr td:nth-child(10)');
  return ageCell?.textContent?.trim() ?? '';
}

function getFirstStrategyId(container: HTMLElement): string {
  return container.querySelector('tbody tr td:first-child')?.textContent?.trim() ?? '';
}

async function flushRender() {
  await act(async () => {
    await Promise.resolve();
  });
}

describe('SignalTable age ticking', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    useSignalStore.setState({ rows: [], lastUpdate: undefined });
    observerRegistry.length = 0;
    globalThis.IntersectionObserver = MockIntersectionObserver as unknown as typeof IntersectionObserver;
    mockSocket.connected = true;
    const handlers = (mockSocket as any).__handlers as Record<string, Array<(payload: any) => void>> | undefined;
    if (handlers) {
      Object.keys(handlers).forEach((key) => delete handlers[key]);
    }
  });

  afterEach(() => {
    vi.useRealTimers();
    globalThis.IntersectionObserver = originalIntersectionObserver;
  });

  it('updates Age each second for visible rows', async () => {
    const serverTsMs = 1_000_000;
    (apiModule.api.getSignalStrategies as any).mockResolvedValue({
      strategies: [createStrategy(serverTsMs)],
      server_time: '2025-01-15 12:00:10',
      server_ts_ms: serverTsMs,
    });

    const { container } = render(<SignalTable />);
    await flushRender();

    expect(screen.getByText('visible_age')).toBeInTheDocument();
    act(() => emitVisibility(true));
    expect(getAgeText(container)).toContain('10.0s');

    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(getAgeText(container)).toContain('11.0s');
  });

  it('does not advance hidden rows while not intersecting (regression safety)', async () => {
    const serverTsMs = 2_000_000;
    (apiModule.api.getSignalStrategies as any).mockResolvedValue({
      strategies: [createStrategy(serverTsMs)],
      server_time: '2025-01-15 12:00:10',
      server_ts_ms: serverTsMs,
    });

    const { container } = render(<SignalTable />);
    await flushRender();

    expect(screen.getByText('visible_age')).toBeInTheDocument();
    act(() => emitVisibility(false));

    const initialAge = getAgeText(container);
    expect(initialAge).toContain('10.0s');

    act(() => {
      vi.advanceTimersByTime(3000);
    });
    expect(getAgeText(container)).toContain('10.0s');
  });

  it('updates age sort order over time when sorted by Age', async () => {
    const serverTsMs = 3_000_000;
    const ageDynamic = createAgedStrategy({
      id: 'age_dynamic',
      ageMs: 999_000,
      serverTsMs,
    });
    const ageMissing = createMissingAgeStrategy('age_missing');

    (apiModule.api.getSignalStrategies as any).mockResolvedValue({
      strategies: [ageDynamic, ageMissing],
      server_time: '2025-01-15 12:00:10',
      server_ts_ms: serverTsMs,
    });

    const { container } = render(<SignalTable />);
    await flushRender();

    expect(screen.getByText('age_dynamic')).toBeInTheDocument();
    expect(screen.getByText('age_missing')).toBeInTheDocument();

    act(() => {
      screen.getByText('Age').click();
    });

    expect(getFirstStrategyId(container)).toContain('age_missing');

    act(() => {
      vi.advanceTimersByTime(2_000);
    });

    expect(getFirstStrategyId(container)).toContain('age_dynamic');
  });
});

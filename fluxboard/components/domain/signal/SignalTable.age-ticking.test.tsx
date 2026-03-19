import { act, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
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
const intersectionObserverCtor = vi.fn(() => ({
  observe: vi.fn(),
  disconnect: vi.fn(),
  takeRecords: vi.fn(() => []),
  unobserve: vi.fn(),
}));

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

function renderSignalTable() {
  return render(
    <MemoryRouter>
      <SignalTable />
    </MemoryRouter>
  );
}

describe('SignalTable age ticking', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    useSignalStore.setState({ rows: [], lastUpdate: undefined });
    globalThis.IntersectionObserver = intersectionObserverCtor as unknown as typeof IntersectionObserver;
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

  it('updates Age each second without creating per-row visibility observers', async () => {
    const serverTsMs = 1_000_000;
    (apiModule.api.getSignalStrategies as any).mockResolvedValue({
      strategies: [createStrategy(serverTsMs)],
      server_time: '2025-01-15 12:00:10',
      server_ts_ms: serverTsMs,
    });

    const { container } = renderSignalTable();
    await flushRender();

    expect(screen.getByText('visible_age')).toBeInTheDocument();
    expect(intersectionObserverCtor).not.toHaveBeenCalled();
    expect(getAgeText(container)).toContain('10.0s');

    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(getAgeText(container)).toContain('11.0s');
  });

  it('keeps advancing ages from the shared clock even when no visibility events are emitted', async () => {
    const serverTsMs = 2_000_000;
    (apiModule.api.getSignalStrategies as any).mockResolvedValue({
      strategies: [createStrategy(serverTsMs)],
      server_time: '2025-01-15 12:00:10',
      server_ts_ms: serverTsMs,
    });

    const { container } = renderSignalTable();
    await flushRender();

    expect(screen.getByText('visible_age')).toBeInTheDocument();
    expect(intersectionObserverCtor).not.toHaveBeenCalled();
    expect(getAgeText(container)).toContain('10.0s');

    act(() => {
      vi.advanceTimersByTime(3000);
    });
    expect(getAgeText(container)).toContain('13.0s');
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

    const { container } = renderSignalTable();
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

import React from 'react';
import { act, cleanup, render } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  __resetViewportClockRegistryForTests,
  getViewportClockDebugState,
  useViewportClock,
} from '@/hooks/useViewportClock';

const renderCounts = new Map<string, number>();

function Probe({
  clockKey,
  subscriberId,
  active = true,
}: {
  clockKey: string;
  subscriberId: string;
  active?: boolean;
}) {
  const now = useViewportClock({
    clockKey,
    subscriberId,
    intervalMs: 1_000,
    active,
  });
  renderCounts.set(subscriberId, (renderCounts.get(subscriberId) ?? 0) + 1);
  return React.createElement('output', { 'data-testid': subscriberId }, String(now));
}

describe('useViewportClock', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-03-19T00:00:00.000Z'));
    renderCounts.clear();
    __resetViewportClockRegistryForTests();
  });

  afterEach(() => {
    cleanup();
    __resetViewportClockRegistryForTests();
    vi.useRealTimers();
  });

  it('shares one timer per clock key across subscribers', () => {
    render(
      React.createElement(
        React.Fragment,
        null,
        React.createElement(Probe, { clockKey: 'panel:trades', subscriberId: 'visible-a' }),
        React.createElement(Probe, { clockKey: 'panel:trades', subscriberId: 'visible-b' }),
      ),
    );

    expect(getViewportClockDebugState('panel:trades')).toMatchObject({
      activeSubscriberCount: 2,
      timerCount: 1,
    });

    act(() => {
      vi.advanceTimersByTime(1_000);
    });

    expect(renderCounts.get('visible-a')).toBe(2);
    expect(renderCounts.get('visible-b')).toBe(2);
  });

  it('only fans ticks out to active subscribers', () => {
    const { getByTestId } = render(
      React.createElement(
        React.Fragment,
        null,
        React.createElement(Probe, { clockKey: 'panel:scanners', subscriberId: 'active' }),
        React.createElement(Probe, {
          clockKey: 'panel:scanners',
          subscriberId: 'hidden',
          active: false,
        }),
      ),
    );

    const hiddenBefore = getByTestId('hidden').textContent;

    act(() => {
      vi.advanceTimersByTime(1_000);
    });

    act(() => {
      vi.advanceTimersByTime(1_000);
    });

    expect(renderCounts.get('active')).toBe(3);
    expect(renderCounts.get('hidden')).toBe(1);
    expect(getByTestId('hidden').textContent).toBe(hiddenBefore);
  });

  it('cleans up the shared timer when the last subscriber unmounts', () => {
    const view = render(React.createElement(Probe, { clockKey: 'panel:alerts', subscriberId: 'row-1' }));
    expect(getViewportClockDebugState('panel:alerts')?.timerCount).toBe(1);

    view.unmount();

    expect(getViewportClockDebugState('panel:alerts')).toMatchObject({
      activeSubscriberCount: 0,
      timerCount: 0,
    });
  });
});

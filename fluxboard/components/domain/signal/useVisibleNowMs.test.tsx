import { act, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { __resetViewportClockRegistryForTests, getViewportClockDebugState } from '@/hooks/useViewportClock';
import { useVisibleNowMs } from './useVisibleNowMs';

function Probe({
  nowProvider,
  disabled = false,
  root,
  refreshKey,
}: {
  nowProvider: () => number;
  disabled?: boolean;
  root?: Element | null;
  refreshKey?: unknown;
}) {
  const { nowMs, targetRef } = useVisibleNowMs<HTMLDivElement>({
    intervalMs: 1000,
    nowProvider,
    disabled,
    root,
    refreshKey,
  });

  return (
    <div ref={targetRef} data-testid="probe">
      {nowMs}
    </div>
  );
}

describe('useVisibleNowMs', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    __resetViewportClockRegistryForTests();
  });

  afterEach(() => {
    __resetViewportClockRegistryForTests();
    vi.useRealTimers();
  });

  it('ticks once per second while active', () => {
    let now = 1_000;
    render(<Probe nowProvider={() => now} />);

    expect(screen.getByTestId('probe')).toHaveTextContent('1000');

    act(() => {
      now = 2_000;
      vi.advanceTimersByTime(1000);
    });

    expect(screen.getByTestId('probe')).toHaveTextContent('2000');
  });

  it('pauses ticking when disabled and resumes when re-enabled', () => {
    let now = 5_000;
    const stableNowProvider = () => now;
    const { rerender } = render(<Probe nowProvider={stableNowProvider} disabled />);

    expect(screen.getByTestId('probe')).toHaveTextContent('5000');

    act(() => {
      now = 6_000;
      vi.advanceTimersByTime(1000);
    });

    expect(screen.getByTestId('probe')).toHaveTextContent('5000');

    act(() => {
      rerender(<Probe nowProvider={stableNowProvider} />);
    });

    act(() => {
      now = 7_000;
      vi.advanceTimersByTime(1000);
    });

    expect(screen.getByTestId('probe')).toHaveTextContent('7000');
  });

  it('shares a single viewport clock across subscribers', () => {
    let now = 1_000;

    render(
      <>
        <Probe nowProvider={() => now} />
        <Probe nowProvider={() => now} />
      </>,
    );

    const debugState = getViewportClockDebugState('signal:visible-now-ms');
    expect(debugState?.activeSubscriberCount).toBe(2);
    expect(debugState?.timerCount).toBe(1);
  });

  it('accepts explicit root without changing ticker behavior', () => {
    let now = 1_000;
    const root = document.createElement('div');
    document.body.appendChild(root);

    render(<Probe nowProvider={() => now} root={root} />);
    expect(screen.getByTestId('probe')).toHaveTextContent('1000');

    act(() => {
      now = 2_000;
      vi.advanceTimersByTime(1000);
    });

    expect(screen.getByTestId('probe')).toHaveTextContent('2000');
  });

  it('recomputes immediately when refreshKey changes and nowProvider identity stays stable', () => {
    let now = 1_000;
    const stableNowProvider = () => now;
    const { rerender } = render(<Probe nowProvider={stableNowProvider} refreshKey="initial" />);

    expect(screen.getByTestId('probe')).toHaveTextContent('1000');

    act(() => {
      now = 2_000;
      rerender(<Probe nowProvider={stableNowProvider} refreshKey="synced" />);
    });

    expect(screen.getByTestId('probe')).toHaveTextContent('2000');
  });
});

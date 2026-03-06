import { act, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useVisibleNowMs } from './useVisibleNowMs';

type MockObserverInstance = {
  emit: (isIntersecting: boolean) => void;
  root: Element | Document | null;
};

const observers: MockObserverInstance[] = [];
const originalIntersectionObserver = globalThis.IntersectionObserver;

class MockIntersectionObserver implements IntersectionObserver {
  readonly root: Element | Document | null;
  readonly rootMargin = '0px';
  readonly thresholds = [0];
  private readonly callback: IntersectionObserverCallback;
  private readonly elements = new Set<Element>();

  constructor(callback: IntersectionObserverCallback, options?: IntersectionObserverInit) {
    this.callback = callback;
    this.root = options?.root ?? null;
    observers.push(this);
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

function Probe({
  nowProvider,
  root,
}: {
  nowProvider: () => number;
  root?: Element | null;
}) {
  const { nowMs, targetRef } = useVisibleNowMs<HTMLDivElement>({
    intervalMs: 1000,
    nowProvider,
    root,
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
    observers.length = 0;
    globalThis.IntersectionObserver = MockIntersectionObserver as unknown as typeof IntersectionObserver;
  });

  afterEach(() => {
    vi.useRealTimers();
    globalThis.IntersectionObserver = originalIntersectionObserver;
  });

  it('ticks once per second while visible', () => {
    let now = 1_000;
    render(<Probe nowProvider={() => now} />);

    const observer = observers[0] as MockIntersectionObserver;
    act(() => {
      observer.emit(true);
    });

    expect(screen.getByTestId('probe')).toHaveTextContent('1000');

    act(() => {
      now = 2_000;
      vi.advanceTimersByTime(1000);
    });

    expect(screen.getByTestId('probe')).toHaveTextContent('2000');
  });

  it('pauses ticking when hidden and resumes when visible again', () => {
    let now = 5_000;
    render(<Probe nowProvider={() => now} />);

    const observer = observers[0] as MockIntersectionObserver;
    act(() => {
      observer.emit(true);
    });

    act(() => {
      now = 6_000;
      vi.advanceTimersByTime(1000);
    });

    expect(screen.getByTestId('probe')).toHaveTextContent('6000');

    act(() => {
      observer.emit(false);
    });

    act(() => {
      now = 9_000;
      vi.advanceTimersByTime(3000);
    });

    expect(screen.getByTestId('probe')).toHaveTextContent('6000');

    act(() => {
      observer.emit(true);
    });

    act(() => {
      now = 10_000;
      vi.advanceTimersByTime(1000);
    });

    expect(screen.getByTestId('probe')).toHaveTextContent('10000');
  });

  it('falls back to regular ticking when IntersectionObserver is unavailable', () => {
    delete (globalThis as any).IntersectionObserver;
    let now = 10_000;
    render(<Probe nowProvider={() => now} />);

    expect(screen.getByTestId('probe')).toHaveTextContent('10000');

    act(() => {
      now = 11_000;
      vi.advanceTimersByTime(1000);
    });

    expect(screen.getByTestId('probe')).toHaveTextContent('11000');
  });

  it('uses explicit root when provided', () => {
    let now = 1_000;
    const root = document.createElement('div');
    document.body.appendChild(root);

    render(<Probe nowProvider={() => now} root={root} />);

    const observer = observers[0] as MockIntersectionObserver;
    expect(observer.root).toBe(root);
  });

  it('auto-detects nearest scroll parent as observer root', () => {
    let now = 1_000;

    render(
      <div>
        <div
          data-testid="outer-scroll"
          style={{ overflowY: 'auto', maxHeight: '40px' }}
        >
          <Probe nowProvider={() => now} />
        </div>
      </div>
    );

    const observer = observers[0] as MockIntersectionObserver;
    const root = screen.getByTestId('outer-scroll');
    expect(observer.root).toBe(root);
  });
});

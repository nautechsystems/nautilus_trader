// FreshnessIndicator component unit tests

import { render, screen } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { FreshnessIndicator } from '../FreshnessIndicator';

describe('FreshnessIndicator Component', () => {
  beforeEach(() => {
    // Mock Date.now() for consistent testing
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2025-01-15T12:00:00Z'));
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('renders "No data" when lastUpdate is undefined', () => {
    render(<FreshnessIndicator lastUpdate={undefined} />);

    expect(screen.getByText('No data')).toBeInTheDocument();
  });

  it('shows green pulsing dot for live data (< 10s old)', () => {
    const now = Date.now();
    const lastUpdate = now - 5000; // 5 seconds ago

    const { container } = render(<FreshnessIndicator lastUpdate={lastUpdate} />);

    // StatusDot uses jade success color for live status
    const greenDot = container.querySelector('[role="status"]');
    expect(greenDot?.getAttribute('style')).toContain('47, 155, 116');

    // Should have pulse animation
    const hasPulse = greenDot?.classList.contains('animate-pulse');
    expect(hasPulse).toBe(true);
  });

  it('shows red non-pulsing dot for stale data (>= 10s old)', () => {
    const now = Date.now();
    const lastUpdate = now - 15000; // 15 seconds ago

    const { container } = render(<FreshnessIndicator lastUpdate={lastUpdate} />);

    // StatusDot uses danger color for stale status
    const redDot = container.querySelector('[role="status"]');
    expect(redDot?.getAttribute('style')).toContain('198, 76, 88');

    // Should NOT have pulse animation
    const hasPulse = redDot?.classList.contains('animate-pulse');
    expect(hasPulse).toBe(false);
  });

  it('respects custom staleThresholdMs', () => {
    const now = Date.now();
    const lastUpdate = now - 7000; // 7 seconds ago

    // With default 10s threshold: should be green (live)
    const { container: defaultContainer } = render(
      <FreshnessIndicator lastUpdate={lastUpdate} />
    );
    expect(defaultContainer.querySelector('[role="status"]')?.getAttribute('style')).toContain('47, 155, 116');

    // With custom 5s threshold: should be red (stale)
    const { container: customContainer } = render(
      <FreshnessIndicator lastUpdate={lastUpdate} staleThresholdMs={5000} />
    );
    expect(customContainer.querySelector('[role="status"]')?.getAttribute('style')).toContain('198, 76, 88');
  });

  it('formats time ago as seconds for recent data', () => {
    const now = Date.now();
    const lastUpdate = now - 3500; // 3.5 seconds ago

    render(<FreshnessIndicator lastUpdate={lastUpdate} />);

    // Should show "3s" or "4s" depending on rounding
    const timeText = screen.getByText(/[34]s/);
    expect(timeText).toBeInTheDocument();
  });

  it('formats time ago as minutes for older data', () => {
    const now = Date.now();
    const lastUpdate = now - 125000; // 125 seconds = 2m 5s

    render(<FreshnessIndicator lastUpdate={lastUpdate} />);

    // Should show "2m"
    expect(screen.getByText('2m')).toBeInTheDocument();
  });

  it('formats time ago as hours for very old data', () => {
    const now = Date.now();
    const lastUpdate = now - 7200000; // 2 hours

    render(<FreshnessIndicator lastUpdate={lastUpdate} />);

    // Should show "2h"
    expect(screen.getByText('2h')).toBeInTheDocument();
  });

  it('formats time ago as days for ancient data', () => {
    const now = Date.now();
    const lastUpdate = now - 172800000; // 2 days

    render(<FreshnessIndicator lastUpdate={lastUpdate} />);

    // Should show "2d"
    expect(screen.getByText('2d')).toBeInTheDocument();
  });

  it('shows neutral indicator when lastUpdate is undefined', () => {
    const { container } = render(<FreshnessIndicator />);

    // StatusDot uses neutral muted color for loading status
    const neutralDot = container.querySelector('[role="status"]');
    expect(neutralDot?.getAttribute('style')).toContain('128, 131, 139');

    // Should have pulse animation (loading state pulses)
    const hasPulse = neutralDot?.classList.contains('animate-pulse');
    expect(hasPulse).toBe(true);

    expect(screen.getByText('No data')).toBeInTheDocument();
  });

  it('has tooltip with last updated text', () => {
    const now = Date.now();
    const lastUpdate = now - 5000; // 5 seconds ago

    const { container } = render(<FreshnessIndicator lastUpdate={lastUpdate} />);

    const wrapper = container.querySelector('[title^="Last updated"]');
    expect(wrapper).toBeInTheDocument();
  });

  it('uses tabular-nums class for consistent width', () => {
    const now = Date.now();
    const lastUpdate = now - 1000; // 1 second ago

    const { container } = render(<FreshnessIndicator lastUpdate={lastUpdate} />);

    const timeText = container.querySelector('.tabular-nums');
    expect(timeText).toBeInTheDocument();
  });

  it('handles edge case of exactly 0ms age', () => {
    const now = Date.now();
    const lastUpdate = now; // Right now

    const { container } = render(<FreshnessIndicator lastUpdate={lastUpdate} />);

    // Should be green (live)
    expect(container.querySelector('[role="status"]')?.getAttribute('style')).toContain('47, 155, 116');

    // Should show "just now"
    expect(screen.getByText('just now')).toBeInTheDocument();
  });

  it('handles edge case at threshold boundary', () => {
    const now = Date.now();
    const threshold = 10000;

    // Exactly at threshold: should be stale (>= threshold)
    const lastUpdateAtThreshold = now - threshold;

    const { container } = render(
      <FreshnessIndicator lastUpdate={lastUpdateAtThreshold} staleThresholdMs={threshold} />
    );

    expect(container.querySelector('[role="status"]')?.getAttribute('style')).toContain('198, 76, 88');
  });

  it('re-renders when lastUpdate prop changes', () => {
    const now = Date.now();
    const initialUpdate = now - 2000; // 2s ago

    const { rerender, container } = render(
      <FreshnessIndicator lastUpdate={initialUpdate} />
    );

    // Should be green
    expect(container.querySelector('[role="status"]')?.getAttribute('style')).toContain('47, 155, 116');

    // Update to stale data
    const staleUpdate = now - 15000; // 15s ago
    rerender(<FreshnessIndicator lastUpdate={staleUpdate} />);

    // Should now be red
    expect(container.querySelector('[role="status"]')?.getAttribute('style')).toContain('198, 76, 88');
  });
});

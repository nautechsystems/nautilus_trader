import { act, render, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { AlertsTable } from '@/components/domain/alerts/AlertsTable';
import type { Alert } from '@/types';

vi.mock('@/components/shared/TimeAgo', () => ({
  TimeAgo: () => <span>0s</span>,
}));

vi.mock('@/hooks/useMobileLayout', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/hooks/useMobileLayout')>();
  return {
    ...actual,
    useMobileLayout: () => ({ isMobile: false, density: 'desktop' }),
  };
});

function createAlert(overrides: Partial<Alert> = {}): Alert {
  return {
    id: 'alert-1',
    level: 'INFO',
    severity: 'INFO',
    title: 'Test Alert',
    message: 'Test alert message',
    details: {},
    timestamp: Math.floor(Date.now() / 1000),
    ts: Math.floor(Date.now() / 1000),
    ...overrides,
  };
}

describe('AlertsTable timer stability', () => {
  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('keeps auto-dismiss timers armed across equivalent parent rerenders', async () => {
    const dismissSpy = vi.fn();
    const infoAlert = createAlert({ id: 'info-1', level: 'INFO', severity: 'INFO' });

    const { rerender } = render(
      <AlertsTable
        alerts={[infoAlert]}
        loading={false}
        dismissedIds={new Set()}
        levelFilter="ALL"
        onDismiss={dismissSpy}
        onRowClick={vi.fn()}
        expandedAlertId={null}
      />
    );

    await act(async () => {
      await vi.advanceTimersByTimeAsync(9_900);
    });

    rerender(
      <AlertsTable
        alerts={[infoAlert]}
        loading={false}
        dismissedIds={new Set()}
        levelFilter="ALL"
        onDismiss={dismissSpy}
        onRowClick={vi.fn()}
        expandedAlertId={null}
      />
    );

    await act(async () => {
      await vi.advanceTimersByTimeAsync(150);
    });

    await waitFor(() => {
      expect(dismissSpy).toHaveBeenCalledWith('info-1');
    });
  });
});

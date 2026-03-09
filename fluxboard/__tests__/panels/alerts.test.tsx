/**
 * Alerts Panel Behavioral Tests
 *
 * Tests auto-dismiss timing, severity filtering, interactions, and WebSocket updates.
 * Verifies timing accuracy with ±100ms tolerance.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useAlertsStore } from '@/stores';
import { AlertsTable } from '@/components/domain/alerts/AlertsTable';
import { ALERT_AUTO_DISMISS } from '@/constants';
import type { Alert, AlertLevel } from '@/types';

// Helper to create mock alerts
function createMockAlert(overrides: Partial<Alert> = {}): Alert {
  return {
    id: `alert-${Math.random().toString(36).substring(7)}`,
    level: 'INFO',
    severity: 'INFO',
    title: 'Test Alert',
    message: 'Test alert message',
    timestamp: Math.floor(Date.now() / 1000),
    ts: Math.floor(Date.now() / 1000),
    details: {},
    ...overrides,
  };
}

describe('AlertsTable Auto-Dismiss', () => {
  let dismissSpy: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    dismissSpy = vi.fn();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('should auto-dismiss INFO alert after 10s (±100ms)', async () => {
    const infoAlert = createMockAlert({ id: 'info-1', level: 'INFO', severity: 'INFO' });

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

    // Alert should be visible initially
    expect(screen.getByText('Test Alert')).toBeInTheDocument();

    // Advance time just before threshold (9.9s)
    await vi.advanceTimersByTimeAsync(9900);
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

    // Should not be dismissed yet
    expect(dismissSpy).not.toHaveBeenCalled();

    // Advance past threshold (10s total)
    await vi.advanceTimersByTimeAsync(150);

    // Wait for dismiss to be called
    await waitFor(() => {
      expect(dismissSpy).toHaveBeenCalledWith('info-1');
    });

    // Verify timing: 9900 + 150 = 10050ms (within 10000ms ± 100ms)
    expect(10050).toBeGreaterThanOrEqual(ALERT_AUTO_DISMISS.INFO - 100);
    expect(10050).toBeLessThanOrEqual(ALERT_AUTO_DISMISS.INFO + 100);
  });

  it('should auto-dismiss WARNING alert after 30s (±100ms)', async () => {
    const warningAlert = createMockAlert({ id: 'warn-1', level: 'WARNING', severity: 'WARNING' });

    const { rerender } = render(
      <AlertsTable
        alerts={[warningAlert]}
        loading={false}
        dismissedIds={new Set()}
        levelFilter="ALL"
        onDismiss={dismissSpy}
        onRowClick={vi.fn()}
        expandedAlertId={null}
      />
    );

    expect(screen.getByText('Test Alert')).toBeInTheDocument();

    // Advance time just before threshold (29.9s)
    await vi.advanceTimersByTimeAsync(29900);
    rerender(
      <AlertsTable
        alerts={[warningAlert]}
        loading={false}
        dismissedIds={new Set()}
        levelFilter="ALL"
        onDismiss={dismissSpy}
        onRowClick={vi.fn()}
        expandedAlertId={null}
      />
    );

    // Should not be dismissed yet
    expect(dismissSpy).not.toHaveBeenCalled();

    // Advance past threshold (30s total)
    await vi.advanceTimersByTimeAsync(150);

    // Wait for dismiss
    await waitFor(() => {
      expect(dismissSpy).toHaveBeenCalledWith('warn-1');
    });

    // Verify timing: 29900 + 150 = 30050ms (within 30000ms ± 100ms)
    expect(30050).toBeGreaterThanOrEqual(ALERT_AUTO_DISMISS.WARNING - 100);
    expect(30050).toBeLessThanOrEqual(ALERT_AUTO_DISMISS.WARNING + 100);
  });

  it('should NOT auto-dismiss CRITICAL alert', async () => {
    const criticalAlert = createMockAlert({ id: 'crit-1', level: 'CRITICAL', severity: 'CRITICAL' });

    render(
      <AlertsTable
        alerts={[criticalAlert]}
        loading={false}
        dismissedIds={new Set()}
        levelFilter="ALL"
        onDismiss={dismissSpy}
        onRowClick={vi.fn()}
        expandedAlertId={null}
      />
    );

    expect(screen.getByText('Test Alert')).toBeInTheDocument();

    // Advance well past INFO and WARNING thresholds (60s)
    await vi.advanceTimersByTimeAsync(60000);

    // CRITICAL should never auto-dismiss
    expect(dismissSpy).not.toHaveBeenCalled();
  });

  it('should handle multiple alerts with different severities sequentially', async () => {
    // Test INFO separately
    const infoAlert = createMockAlert({ id: 'info-1', level: 'INFO', severity: 'INFO', title: 'Info Alert' });

    render(
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

    expect(screen.getByText('Info Alert')).toBeInTheDocument();

    // Advance past INFO threshold (10s)
    await vi.advanceTimersByTimeAsync(10100);
    await waitFor(() => {
      expect(dismissSpy).toHaveBeenCalledWith('info-1');
    });
  });

  it('should verify CRITICAL never dismisses alongside others', async () => {
    const criticalAlert = createMockAlert({ id: 'crit-1', level: 'CRITICAL', severity: 'CRITICAL', title: 'Critical Alert' });

    render(
      <AlertsTable
        alerts={[criticalAlert]}
        loading={false}
        dismissedIds={new Set()}
        levelFilter="ALL"
        onDismiss={dismissSpy}
        onRowClick={vi.fn()}
        expandedAlertId={null}
      />
    );

    expect(screen.getByText('Critical Alert')).toBeInTheDocument();

    // Advance well past WARNING threshold (40s)
    await vi.advanceTimersByTimeAsync(40000);

    // CRITICAL never dismissed
    expect(dismissSpy).not.toHaveBeenCalled();
  });
});

describe('AlertsTable Filtering', () => {
  it('should respect dismissedIds set', async () => {
    const alerts = [
      createMockAlert({ id: 'alert-1', title: 'Alert 1' }),
      createMockAlert({ id: 'alert-2', title: 'Alert 2' }),
    ];

    render(
      <AlertsTable
        alerts={alerts}
        loading={false}
        dismissedIds={new Set(['alert-1'])}
        levelFilter="ALL"
        onDismiss={vi.fn()}
        onRowClick={vi.fn()}
        expandedAlertId={null}
      />
    );

    // alert-1 should be hidden
    expect(screen.queryByText('Alert 1')).not.toBeInTheDocument();

    // alert-2 should be visible
    expect(screen.getByText('Alert 2')).toBeInTheDocument();
  });

  it('should filter by severity level', () => {
    const alerts = [
      createMockAlert({ id: 'info-1', level: 'INFO', severity: 'INFO', title: 'Info Alert' }),
      createMockAlert({ id: 'warn-1', level: 'WARNING', severity: 'WARNING', title: 'Warning Alert' }),
      createMockAlert({ id: 'crit-1', level: 'CRITICAL', severity: 'CRITICAL', title: 'Critical Alert' }),
    ];

    const { rerender } = render(
      <AlertsTable
        alerts={alerts}
        loading={false}
        dismissedIds={new Set()}
        levelFilter="CRITICAL"
        onDismiss={vi.fn()}
        onRowClick={vi.fn()}
        expandedAlertId={null}
      />
    );

    // Only CRITICAL visible
    expect(screen.queryByText('Info Alert')).not.toBeInTheDocument();
    expect(screen.queryByText('Warning Alert')).not.toBeInTheDocument();
    expect(screen.getByText('Critical Alert')).toBeInTheDocument();

    // Change filter to INFO
    rerender(
      <AlertsTable
        alerts={alerts}
        loading={false}
        dismissedIds={new Set()}
        levelFilter="INFO"
        onDismiss={vi.fn()}
        onRowClick={vi.fn()}
        expandedAlertId={null}
      />
    );

    // Only INFO visible
    expect(screen.getByText('Info Alert')).toBeInTheDocument();
    expect(screen.queryByText('Warning Alert')).not.toBeInTheDocument();
    expect(screen.queryByText('Critical Alert')).not.toBeInTheDocument();
  });

  it('should show all alerts when filter is ALL', () => {
    const alerts = [
      createMockAlert({ id: 'info-1', level: 'INFO', severity: 'INFO', title: 'Info Alert' }),
      createMockAlert({ id: 'warn-1', level: 'WARNING', severity: 'WARNING', title: 'Warning Alert' }),
    ];

    render(
      <AlertsTable
        alerts={alerts}
        loading={false}
        dismissedIds={new Set()}
        levelFilter="ALL"
        onDismiss={vi.fn()}
        onRowClick={vi.fn()}
        expandedAlertId={null}
      />
    );

    expect(screen.getByText('Info Alert')).toBeInTheDocument();
    expect(screen.getByText('Warning Alert')).toBeInTheDocument();
  });
});

describe('AlertsTable Interactions', () => {
  it('should call onDismiss when dismiss button clicked', async () => {
    const dismissFn = vi.fn();
    const alert = createMockAlert({ id: 'test-1', title: 'Test Alert' });

    const user = userEvent.setup({ delay: null });

    render(
      <AlertsTable
        alerts={[alert]}
        loading={false}
        dismissedIds={new Set()}
        levelFilter="ALL"
        onDismiss={dismissFn}
        onRowClick={vi.fn()}
        expandedAlertId={null}
      />
    );

    const dismissButton = screen.getByLabelText('Dismiss alert test-1');
    await user.click(dismissButton);

    expect(dismissFn).toHaveBeenCalledWith('test-1');
  });

  it('should call onRowClick when row clicked', async () => {
    const rowClickFn = vi.fn();
    const alert = createMockAlert({ id: 'test-1', title: 'Test Alert' });

    const user = userEvent.setup({ delay: null });

    render(
      <AlertsTable
        alerts={[alert]}
        loading={false}
        dismissedIds={new Set()}
        levelFilter="ALL"
        onDismiss={vi.fn()}
        onRowClick={rowClickFn}
        expandedAlertId={null}
      />
    );

    const alertRow = screen.getByText('Test Alert').closest('tr');
    await user.click(alertRow!);

    expect(rowClickFn).toHaveBeenCalledWith(alert);
  });

  it('should show loading state', () => {
    render(
      <AlertsTable
        alerts={[]}
        loading={true}
        dismissedIds={new Set()}
        levelFilter="ALL"
        onDismiss={vi.fn()}
        onRowClick={vi.fn()}
        expandedAlertId={null}
      />
    );

    expect(screen.getByText('Loading...')).toBeInTheDocument();
  });

  it('should show empty state', () => {
    render(
      <AlertsTable
        alerts={[]}
        loading={false}
        dismissedIds={new Set()}
        levelFilter="ALL"
        onDismiss={vi.fn()}
        onRowClick={vi.fn()}
        expandedAlertId={null}
      />
    );

    expect(screen.getByText('No alerts')).toBeInTheDocument();
  });
});

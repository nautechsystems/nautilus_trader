import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { AlertsTable } from '@/components/domain/alerts/AlertsTable';
import type { Alert } from '@/types';

function createAlert(overrides: Partial<Alert> = {}): Alert {
  const now = Math.floor(Date.now() / 1000);
  return {
    id: 'alert-1',
    level: 'INFO',
    severity: 'INFO',
    title: 'Alert Title',
    message: 'Alert message',
    details: {},
    timestamp: now,
    ts: now,
    ...overrides,
  };
}

describe('AlertsTable typography', () => {
  it('uses standardized header typography classes', () => {
    const alert = createAlert();

    render(
      <AlertsTable
        alerts={[alert]}
        loading={false}
        dismissedIds={new Set()}
        levelFilter="ALL"
        onDismiss={vi.fn()}
        onRowClick={vi.fn()}
        expandedAlertId={null}
      />
    );

    const ageHeader = screen.getByRole('columnheader', { name: 'Age' });
    expect(ageHeader).toHaveClass('text-xs', 'font-semibold', 'uppercase');
  });

  it('uses DataTable-aligned body typography in normal and dense modes', () => {
    const alert = createAlert();

    const { container, rerender } = render(
      <AlertsTable
        alerts={[alert]}
        loading={false}
        dismissedIds={new Set()}
        levelFilter="ALL"
        onDismiss={vi.fn()}
        onRowClick={vi.fn()}
        expandedAlertId={null}
        dense={false}
      />
    );

    const normalBodyCell = container.querySelector('tbody td');
    expect(normalBodyCell).toHaveStyle({ fontSize: '13px', fontWeight: '400' });

    rerender(
      <AlertsTable
        alerts={[alert]}
        loading={false}
        dismissedIds={new Set()}
        levelFilter="ALL"
        onDismiss={vi.fn()}
        onRowClick={vi.fn()}
        expandedAlertId={null}
        dense
      />
    );

    const denseBodyCell = container.querySelector('tbody td');
    expect(denseBodyCell).toHaveStyle({ fontSize: '12px', fontWeight: '400' });
  });
});

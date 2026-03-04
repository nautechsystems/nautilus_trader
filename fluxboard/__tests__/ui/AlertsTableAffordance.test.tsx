import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { ComponentProps } from 'react';
import { describe, expect, it, vi } from 'vitest';
import { AlertsTable } from '@/components/domain/alerts/AlertsTable';
import type { Alert } from '@/types';

vi.mock('@/hooks/useMobileLayout', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/hooks/useMobileLayout')>();
  return {
    ...actual,
    useMobileLayout: () => ({ isMobile: false }),
  };
});

const baseAlert: Alert = {
  id: 'alert-1',
  level: 'CRITICAL',
  severity: 'CRITICAL',
  title: 'RPC disconnected',
  message: 'RPC disconnected',
  strategy_id: 'alpha',
  timestamp: 1739251200,
  details: { reason: 'upstream timeout' },
};

function renderTable(overrides: Partial<ComponentProps<typeof AlertsTable>> = {}) {
  const onDismiss = vi.fn();
  const onRowClick = vi.fn();

  render(
    <AlertsTable
      alerts={[baseAlert]}
      loading={false}
      dismissedIds={new Set()}
      levelFilter="ALL"
      onDismiss={onDismiss}
      onRowClick={onRowClick}
      expandedAlertId={null}
      dense={false}
      {...overrides}
    />
  );

  return { onDismiss, onRowClick };
}

describe('AlertsTable affordance semantics', () => {
  it('renders an explicit expand control with aria-expanded state', () => {
    renderTable();

    const toggle = screen.getByRole('button', { name: /expand details for alert alert-1/i });
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
  });

  it('marks expanded row state persistently when details are open', () => {
    renderTable({ expandedAlertId: 'alert-1' });

    const toggle = screen.getByRole('button', { name: /collapse details for alert alert-1/i });
    expect(toggle).toHaveAttribute('aria-expanded', 'true');

    const row = toggle.closest('tr');
    expect(row).toHaveAttribute('data-expanded', 'true');
    expect(row).toHaveClass('alert-row--expanded');
  });

  it('uses explicit control to toggle details without dismissing', async () => {
    const user = userEvent.setup();
    const { onDismiss, onRowClick } = renderTable();

    const toggle = screen.getByRole('button', { name: /expand details for alert alert-1/i });
    await user.click(toggle);

    expect(onRowClick).toHaveBeenCalledWith(expect.objectContaining({ id: 'alert-1' }));
    expect(onDismiss).not.toHaveBeenCalled();
  });

  it('dismiss action does not toggle expansion', async () => {
    const user = userEvent.setup();
    const { onDismiss, onRowClick } = renderTable();

    const dismiss = screen.getByRole('button', { name: /dismiss alert alert-1/i });
    await user.click(dismiss);

    expect(onDismiss).toHaveBeenCalledWith('alert-1');
    expect(onRowClick).not.toHaveBeenCalled();
  });
});

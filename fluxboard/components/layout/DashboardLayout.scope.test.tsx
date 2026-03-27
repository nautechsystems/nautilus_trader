import { render } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { DashboardLayout } from './DashboardLayout';
import * as storage from '../../utils/storage';

vi.mock('react-grid-layout', () => ({
  Responsive: ({ children }: { children: React.ReactNode }) => (
    <div data-testid="grid-layout">{children}</div>
  ),
}));

vi.mock('../../utils/storage', () => ({
  saveLayout: vi.fn(),
  loadLayout: vi.fn(() => ({
    lg: [{ i: 'signal', x: 0, y: 0, w: 12, h: 3 }],
  })),
  createLayoutsFromPreset: vi.fn(() => ({
    lg: [
      { i: 'signal', x: 0, y: 0, w: 12, h: 3 },
      { i: 'balances', x: 0, y: 3, w: 12, h: 3 },
    ],
  })),
  saveCollapsedPanels: vi.fn(),
  loadCollapsedPanels: vi.fn(() => new Set()),
}));

vi.mock('./PanelRegistry', () => ({
  PANEL_REGISTRY: {
    signal: () => <div>Signal</div>,
    balances: () => <div>Balances</div>,
  },
}));

describe('DashboardLayout storage scope', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('loads dashboard state using the provided surface scope', () => {
    render(<DashboardLayout {...({ storageScope: 'equities' } as any)} />);

    expect(storage.loadLayout).toHaveBeenCalledWith('default', 'equities');
    expect(storage.loadCollapsedPanels).toHaveBeenCalledWith('equities');
  });
});

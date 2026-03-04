import { render, screen } from '@testing-library/react';
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { DashboardLayout } from './DashboardLayout';

// --- Mocks ---
let mobileFlag = true;

vi.mock('@/hooks/useIsMobile', () => ({
  useIsMobile: () => mobileFlag,
}));

vi.mock('./MobileDashboard', () => ({
  MobileDashboard: () => <div data-testid="mobile-dashboard" />,
}));

// Keep desktop branch lightweight with stubbed grid layout and panels
vi.mock('react-grid-layout', () => ({
  Responsive: ({ children }: any) => <div data-testid="grid-layout">{children}</div>,
}));

vi.mock('../../utils/storage', () => ({
  saveLayout: vi.fn(),
  loadLayout: vi.fn(() => ({ lg: [{ i: 'trades', x: 0, y: 0, w: 12, h: 3 }] })),
  createLayoutsFromPreset: vi.fn(() => ({ lg: [{ i: 'trades', x: 0, y: 0, w: 12, h: 3 }] })),
  saveCollapsedPanels: vi.fn(),
  loadCollapsedPanels: vi.fn(() => new Set()),
}));

vi.mock('./PanelRegistry', () => ({
  PANEL_REGISTRY: { trades: () => <div data-testid="panel-trades" /> },
}));

vi.mock('./presets', () => ({ PRESETS: { default: [] } }));

// --- Tests ---
describe('DashboardLayout mobile switch', () => {
  beforeEach(() => {
    mobileFlag = true;
  });

  it('renders MobileDashboard when useIsMobile returns true', () => {
    render(<DashboardLayout />);
    expect(screen.getByTestId('mobile-dashboard')).toBeInTheDocument();
    expect(screen.queryByTestId('grid-layout')).not.toBeInTheDocument();
  });

  it('renders desktop grid layout when useIsMobile is false', () => {
    mobileFlag = false;
    render(<DashboardLayout />);
    expect(screen.getByTestId('grid-layout')).toBeInTheDocument();
    expect(screen.queryByTestId('mobile-dashboard')).not.toBeInTheDocument();
  });
});

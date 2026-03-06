// DashboardLayout tests

import { render, screen, fireEvent, act } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { DashboardLayout } from './DashboardLayout';
import * as storage from '../../utils/storage';

// Mock react-grid-layout
declare global {
  // eslint-disable-next-line no-var
  var __dashboardResponsiveMock: {
    triggerLayoutChange: (layoutOverride?: any[], layoutsOverride?: Record<string, any>) => void;
    triggerResizeStop: (layoutOverride?: any[]) => void;
  } | undefined;
}

vi.mock('react-grid-layout', () => {
  const bridge: { props?: any } = {};

  const Responsive = ({ children, layouts, breakpoints, cols, width, ...rest }: any) => {
    bridge.props = {
      layouts,
      breakpoints,
      cols,
      width,
      ...rest,
    };

    return (
      <div
        data-testid="grid-layout"
        data-layouts={JSON.stringify(layouts)}
        data-breakpoints={JSON.stringify(breakpoints)}
        data-cols={JSON.stringify(cols)}
        data-width={width}
      >
        {children}
      </div>
    );
  };

  (globalThis as any).__dashboardResponsiveMock = {
    triggerLayoutChange(layoutOverride?: any[], layoutsOverride?: Record<string, any>) {
      const props = bridge.props;
      if (!props?.onLayoutChange) return;
      const layout = layoutOverride ?? props.layouts?.lg ?? [];
      const layouts = layoutsOverride ?? props.layouts;
      props.onLayoutChange(layout, layouts);
    },
    triggerResizeStop(layoutOverride?: any[]) {
      const props = bridge.props;
      if (!props?.onResizeStop) return;
      const layout = layoutOverride ?? props.layouts?.lg ?? [];
      const mockItem = { i: 'signal', x: 0, y: 0, w: 12, h: 3 };
      props.onResizeStop(layout, mockItem, mockItem, null, new Event('mouseup'), document.createElement('div'));
    },
  };

  return {
    Responsive,
  };
});

// Mock storage utils
vi.mock('../../utils/storage', () => ({
  saveLayout: vi.fn(),
  loadLayout: vi.fn(() => ({
    lg: [
      { i: 'signal', x: 0, y: 0, w: 12, h: 3 },
      { i: 'trades', x: 0, y: 3, w: 12, h: 3 }
    ]
  })),
  createLayoutsFromPreset: vi.fn(() => ({
    lg: [
      { i: 'signal', x: 0, y: 0, w: 12, h: 3 },
      { i: 'trades', x: 0, y: 3, w: 12, h: 3 }
    ]
  })),
  saveCollapsedPanels: vi.fn(),
  loadCollapsedPanels: vi.fn(() => new Set())
}));

// Mock panel registry with components that accept fullWidth/collapsed props
vi.mock('./PanelRegistry', () => {
  const MockPanel = ({ fullWidth }: { fullWidth?: boolean; collapsed?: boolean }) => (
    <div data-testid="mock-panel" data-full-width={fullWidth ? 'true' : 'false'}>
      Mock Panel
    </div>
  );

  return {
    PANEL_REGISTRY: {
      signal: MockPanel,
      trades: MockPanel,
      balances: MockPanel
    }
  };
});

// Mock presets
vi.mock('./presets', () => ({
  PRESETS: {
    default: []
  }
}));

describe('DashboardLayout', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders without crashing', () => {
    render(<DashboardLayout />);
    expect(screen.getByTestId('grid-layout')).toBeInTheDocument();
  });

  it('uses single overflow container instead of nested', () => {
    const { container } = render(<DashboardLayout />);

    // Should have single overflow-auto container
    const overflowContainers = container.querySelectorAll('.overflow-auto');
    expect(overflowContainers.length).toBe(1);

    // Should NOT have overflow-hidden on same container
    const gridContainer = container.querySelector('.flex-1.overflow-auto');
    expect(gridContainer).toBeInTheDocument();
    expect(gridContainer?.classList.contains('overflow-hidden')).toBe(false);
  });

  it('uses clean flex layout without redundant classes', () => {
    const { container } = render(<DashboardLayout />);

    const root = container.firstChild as HTMLElement;
    expect(root.classList.contains('flex')).toBe(true);
    expect(root.classList.contains('flex-col')).toBe(true);
    expect(root.classList.contains('h-full')).toBe(true);
    expect(root.classList.contains('dashboard-root')).toBe(true);

    // Should NOT have redundant classes
    expect(root.classList.contains('overflow-hidden')).toBe(false);
    expect(root.classList.contains('max-w-none')).toBe(false);
    expect(root.classList.contains('mx-0')).toBe(false);
    expect(root.classList.contains('px-0')).toBe(false);
  });

  it('renders add panel buttons for available panels', () => {
    render(<DashboardLayout />);

    // Should show "Add" label
    expect(screen.getByText('Add:')).toBeInTheDocument();

    // Should show button for balances (not in initial layout)
    expect(screen.getByText('+ balances')).toBeInTheDocument();
  });

  it('respects allowedPanels and strips disallowed panels from layout', () => {
    vi.mocked(storage.loadLayout).mockReturnValueOnce({
      lg: [
        { i: 'signal', x: 0, y: 0, w: 12, h: 3 },
        { i: 'trades', x: 0, y: 3, w: 12, h: 3 },
        { i: 'balances', x: 0, y: 6, w: 12, h: 3 },
      ],
    });

    render(<DashboardLayout allowedPanels={['signal', 'trades']} />);

    expect(screen.queryByText('+ balances')).not.toBeInTheDocument();

    const gridLayout = screen.getByTestId('grid-layout');
    const layoutData = JSON.parse(gridLayout.getAttribute('data-layouts') || '{}');
    expect(layoutData.lg.map((item: { i: string }) => item.i)).toEqual(['signal', 'trades']);
  });

  it('adds aria-label to panel buttons for accessibility', () => {
    render(<DashboardLayout />);

    const addButton = screen.getByText('+ balances').closest('button');
    expect(addButton).toHaveAttribute('aria-label', 'Add balances panel');
  });

  it('handles panel collapse state correctly', () => {
    render(<DashboardLayout />);

    // Initial layout should render
    expect(screen.getByTestId('grid-layout')).toBeInTheDocument();

    const gridLayout = screen.getByTestId('grid-layout');
    const layoutData = JSON.parse(gridLayout.getAttribute('data-layouts') || '{}');

    // Verify layout structure exists
    expect(Array.isArray(layoutData?.lg)).toBe(true);
    expect(layoutData.lg.length).toBeGreaterThan(0);
  });

  it('removes panel selector header when no panels available', () => {
    // Mock all panels as active
    vi.mocked(storage.loadLayout).mockReturnValueOnce({
      lg: [
        { i: 'signal', x: 0, y: 0, w: 12, h: 3 },
        { i: 'trades', x: 0, y: 3, w: 12, h: 3 },
        { i: 'balances', x: 0, y: 6, w: 12, h: 3 }
      ]
    });

    render(<DashboardLayout />);

    // Should NOT show "Add:" label when all panels active
    expect(screen.queryByText('Add:')).not.toBeInTheDocument();
  });

  it('passes correct props to GridLayout', () => {
    const { container } = render(<DashboardLayout />);

    const gridLayout = container.querySelector('[data-testid="grid-layout"]');
    expect(gridLayout).toBeInTheDocument();

    // Verify layout data is passed
    const layoutData = JSON.parse(gridLayout?.getAttribute('data-layouts') || '{}');
    expect(layoutData.lg).toEqual(expect.arrayContaining([
      expect.objectContaining({ i: 'signal' }),
      expect.objectContaining({ i: 'trades' })
    ]));
  });

  it('applies scoped dashboard class to root for compact styling', () => {
    const { container } = render(<DashboardLayout />);
    const root = container.firstChild as HTMLElement;
    expect(root).toBeTruthy();
    expect(root.className).toMatch(/dashboard-root/);
  });

  describe('fullWidth prop handling', () => {
    it('passes fullWidth=true to panels with width 12', () => {
      // Mock layout with full-width panels (w=12)
      vi.mocked(storage.loadLayout).mockReturnValueOnce({
        lg: [
          { i: 'signal', x: 0, y: 0, w: 12, h: 3 },
          { i: 'trades', x: 0, y: 3, w: 12, h: 3 }
        ]
      });

      render(<DashboardLayout />);

      const panels = screen.getAllByTestId('mock-panel');
      expect(panels.length).toBe(2);

      // Both panels should have fullWidth=true (w=12)
      panels.forEach(panel => {
        expect(panel.getAttribute('data-full-width')).toBe('true');
      });
    });

    it('passes fullWidth=false to panels with width less than 12', () => {
      // Mock layout with non-full-width panels (w < 12)
      vi.mocked(storage.loadLayout).mockReturnValueOnce({
        lg: [
          { i: 'signal', x: 0, y: 0, w: 6, h: 3 },
          { i: 'trades', x: 6, y: 0, w: 6, h: 3 },
          { i: 'balances', x: 0, y: 3, w: 8, h: 3 }
        ]
      });

      render(<DashboardLayout />);

      const panels = screen.getAllByTestId('mock-panel');
      expect(panels.length).toBe(3);

      // All panels should have fullWidth=false (w < 12)
      panels.forEach(panel => {
        expect(panel.getAttribute('data-full-width')).toBe('false');
      });
    });

    it('correctly identifies fullWidth based on panel width in mixed layout', () => {
      // Mock layout with mix of full-width and non-full-width panels
      vi.mocked(storage.loadLayout).mockReturnValueOnce({
        lg: [
          { i: 'signal', x: 0, y: 0, w: 12, h: 3 },  // fullWidth=true
          { i: 'trades', x: 0, y: 3, w: 6, h: 3 },   // fullWidth=false
          { i: 'balances', x: 6, y: 3, w: 6, h: 3 }  // fullWidth=false
        ]
      });

      render(<DashboardLayout />);

      const panels = screen.getAllByTestId('mock-panel');
      expect(panels.length).toBe(3);

      // First panel (signal) should be fullWidth=true
      expect(panels[0].getAttribute('data-full-width')).toBe('true');

      // Other panels should be fullWidth=false
      expect(panels[1].getAttribute('data-full-width')).toBe('false');
      expect(panels[2].getAttribute('data-full-width')).toBe('false');
    });

  it('passes fullWidth based on current panel width in layout state', () => {
    // Test with a panel that is not full-width
    vi.mocked(storage.loadLayout).mockReturnValueOnce({
      lg: [
        { i: 'signal', x: 0, y: 0, w: 6, h: 3 },
        { i: 'trades', x: 6, y: 0, w: 6, h: 3 }
      ]
    });

    render(<DashboardLayout />);

    const panels = screen.getAllByTestId('mock-panel');
    expect(panels.length).toBe(2);

    // Both panels should have fullWidth=false (w < 12)
    panels.forEach(panel => {
      expect(panel.getAttribute('data-full-width')).toBe('false');
    });
  });

  it('exits resize mode cleanly and saves layout when resize stops', () => {
    render(<DashboardLayout />);

    expect(globalThis.__dashboardResponsiveMock).toBeDefined();

    const resizedLayout = [
      { i: 'signal', x: 0, y: 0, w: 6, h: 3 },
      { i: 'trades', x: 6, y: 0, w: 6, h: 3 },
    ];
    act(() => {
      globalThis.__dashboardResponsiveMock?.triggerLayoutChange(resizedLayout, { lg: resizedLayout });
    });
    expect(() =>
      act(() => {
        globalThis.__dashboardResponsiveMock?.triggerResizeStop();
      })
    ).not.toThrow();

    expect(storage.saveLayout).toHaveBeenCalledWith(
      'default',
      expect.objectContaining({
        lg: expect.any(Array),
      })
    );
  });

  it('renders a bottom spacer to provide drag-and-resize buffer', () => {
    render(<DashboardLayout />);

    const spacer = screen.getByTestId('dashboard-bottom-spacer');
    expect(spacer).toBeInTheDocument();
    expect(spacer).toHaveStyle({ height: '160px' });
  });

  it('preserves original heights for collapsed panels when layout changes occur', () => {
    vi.mocked(storage.loadLayout).mockReturnValueOnce({
      lg: [
        { i: 'signal', x: 0, y: 0, w: 12, h: 5 },
        { i: 'trades', x: 0, y: 5, w: 12, h: 3 }
      ]
    });
    vi.mocked(storage.loadCollapsedPanels).mockReturnValueOnce(new Set(['signal']));

    render(<DashboardLayout />);

    const collapsedLayout = [
      { i: 'signal', x: 0, y: 0, w: 12, h: 1 },
      { i: 'trades', x: 0, y: 1, w: 12, h: 3 }
    ];

    act(() => {
      globalThis.__dashboardResponsiveMock?.triggerLayoutChange(collapsedLayout, { lg: collapsedLayout });
    });
    act(() => {
      globalThis.__dashboardResponsiveMock?.triggerResizeStop(collapsedLayout);
    });

    expect(storage.saveLayout).toHaveBeenCalledWith(
      'default',
      expect.objectContaining({
        lg: expect.arrayContaining([
          expect.objectContaining({ i: 'signal', h: 5 })
        ])
      })
    );
  });
});
});

// DashboardLayout responsive layout tests
// Tests for breakpoint behavior, horizontal scrolling, and small screen layouts

import { render, screen, waitFor, act } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { DashboardLayout } from './DashboardLayout';
import * as storage from '../../utils/storage';
import { useIsMobile } from '@/hooks/useIsMobile';

vi.mock('@/hooks/useIsMobile', () => ({ useIsMobile: vi.fn(() => false) }));

const resizeCallbacks: ResizeObserverCallback[] = [];
let mockContainerWidth = 1200;

class ResizeObserverMock {
  callback: ResizeObserverCallback;
  constructor(callback: ResizeObserverCallback) {
    this.callback = callback;
    resizeCallbacks.push(callback);
  }
  observe() {}
  unobserve() {}
  disconnect() {
    const idx = resizeCallbacks.indexOf(this.callback);
    if (idx >= 0) {
      resizeCallbacks.splice(idx, 1);
    }
  }
}

Object.defineProperty(window, 'ResizeObserver', {
  writable: true,
  value: ResizeObserverMock,
});

const createRect = (width = 0, height = 0) => ({
  width,
  height,
  top: 0,
  left: 0,
  right: width,
  bottom: height,
  x: 0,
  y: 0,
  toJSON() {
    return {};
  },
});

vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function getBoundingClientRectMock(this: HTMLElement) {
  if (this.classList?.contains('flex-1')) {
    return createRect(mockContainerWidth, 800);
  }
  return createRect();
});

const triggerResize = (width: number) => {
  mockContainerWidth = width;
  resizeCallbacks.forEach(cb => cb([
    {
      target: document.body,
      contentRect: createRect(width, 800),
      borderBoxSize: [],
      contentBoxSize: [],
      devicePixelContentBoxSize: [],
    } as ResizeObserverEntry,
  ], {} as ResizeObserver));
};

// Mock react-grid-layout with responsive behavior
const mockBreakpoint = vi.fn();
const mockOnBreakpointChange = vi.fn();

vi.mock('react-grid-layout', () => {
  const Responsive = ({
    children,
    layouts,
    breakpoints,
    cols,
    onBreakpointChange,
    width,
  }: any) => {
    // Simulate breakpoint change
    if (onBreakpointChange && mockBreakpoint()) {
      const currentWidth = window.innerWidth;
      let bp = 'lg';
      if (currentWidth < 480) bp = 'xxs';
      else if (currentWidth < 768) bp = 'xs';
      else if (currentWidth < 996) bp = 'sm';
      else if (currentWidth < 1200) bp = 'md';
      onBreakpointChange(bp);
    }

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
  loadCollapsedPanels: vi.fn(() => new Set()),
  getSoundMuted: vi.fn(() => false),
  setSoundMuted: vi.fn()
}));

// Mock panel registry
vi.mock('./PanelRegistry', () => {
  const MockPanel = ({ fullWidth }: { fullWidth?: boolean; collapsed?: boolean }) => (
    <div
      data-testid="mock-panel"
      data-full-width={fullWidth ? 'true' : 'false'}
      style={{ width: fullWidth ? '100%' : '1280px', minWidth: '800px' }}
    >
      Mock Panel Content
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

describe('DashboardLayout Responsive Behavior', () => {
  const originalInnerWidth = window.innerWidth;
  const originalInnerHeight = window.innerHeight;

  beforeEach(() => {
    vi.clearAllMocks();
    mockBreakpoint.mockReturnValue(false);
    resizeCallbacks.splice(0, resizeCallbacks.length);
    mockContainerWidth = 1200;
  });

  afterEach(() => {
    Object.defineProperty(window, 'innerWidth', {
      writable: true,
      configurable: true,
      value: originalInnerWidth,
    });
    Object.defineProperty(window, 'innerHeight', {
      writable: true,
      configurable: true,
      value: originalInnerHeight,
    });
    vi.mocked(useIsMobile).mockReturnValue(false);
  });

  const setViewportSize = (width: number, height: number) => {
    act(() => {
      Object.defineProperty(window, 'innerWidth', {
        writable: true,
        configurable: true,
        value: width,
      });
      Object.defineProperty(window, 'innerHeight', {
        writable: true,
        configurable: true,
        value: height,
      });
      // Trigger resize event
      window.dispatchEvent(new Event('resize'));
      triggerResize(width);
    });
  };

  describe('Breakpoint Configuration', () => {
    it('uses correct breakpoints', () => {
      const { container } = render(<DashboardLayout />);
      const gridLayout = container.querySelector('[data-testid="grid-layout"]');
      const breakpoints = JSON.parse(gridLayout?.getAttribute('data-breakpoints') || '{}');

      expect(breakpoints).toEqual({
        lg: 1200,
        md: 996,
        sm: 768,
        xs: 480,
        xxs: 0,
      });
    });

    it('uses correct column counts for each breakpoint', () => {
      const { container } = render(<DashboardLayout />);
      const gridLayout = container.querySelector('[data-testid="grid-layout"]');
      const cols = JSON.parse(gridLayout?.getAttribute('data-cols') || '{}');

      expect(cols).toEqual({
        lg: 12,
        md: 12,
        sm: 6,
        xs: 4,
        xxs: 1,
      });
    });
  });

  describe('Horizontal Scrolling', () => {
    it('has overflow-x: auto on scroll container', () => {
      const { container } = render(<DashboardLayout />);
      const scrollContainer = container.querySelector('.flex-1.overflow-auto');

      expect(scrollContainer).toBeInTheDocument();
      const styles = window.getComputedStyle(scrollContainer as Element);
      expect(styles.overflowX).toBe('auto');
    });

    it('allows content to expand beyond viewport width', () => {
      setViewportSize(800, 600);
      const { container } = render(<DashboardLayout />);

      const scrollContainer = container.querySelector('.flex-1.overflow-auto') as HTMLElement;
      expect(scrollContainer).toBeInTheDocument();

      // Verify container can scroll horizontally
      expect(scrollContainer.style.overflowX).toBe('auto');
    });

    it('has min-width: 0 on flex children to allow shrinking', () => {
      const { container } = render(<DashboardLayout />);
      const scrollContainer = container.querySelector('.flex-1.overflow-auto') as HTMLElement;

      expect(scrollContainer).toBeInTheDocument();
      expect(scrollContainer.style.minWidth).toBe('0');
    });
  });

  describe('Width Tracking', () => {
    it('updates the grid width when the container resizes', async () => {
      act(() => triggerResize(1000));
      const { container } = render(<DashboardLayout />);
      const gridLayout = container.querySelector('[data-testid="grid-layout"]');

      expect(gridLayout?.getAttribute('data-width')).toBe('1000');

      act(() => triggerResize(640));
      await waitFor(() => {
        expect(gridLayout?.getAttribute('data-width')).toBe('640');
      });
    });
  });

  describe('Panel Width Handling', () => {
    it('passes fullWidth=true to panels at full width (w=12)', () => {
      vi.mocked(storage.loadLayout).mockReturnValueOnce({
        lg: [
          { i: 'signal', x: 0, y: 0, w: 12, h: 3 }
        ]
      });

      render(<DashboardLayout />);
      const panel = screen.getByTestId('mock-panel');
      expect(panel.getAttribute('data-full-width')).toBe('true');
    });

    it('passes fullWidth=false to panels at partial width (w<12)', () => {
      vi.mocked(storage.loadLayout).mockReturnValueOnce({
        lg: [
          { i: 'signal', x: 0, y: 0, w: 6, h: 3 }
        ]
      });

      render(<DashboardLayout />);
      const panel = screen.getByTestId('mock-panel');
      expect(panel.getAttribute('data-full-width')).toBe('false');
    });
  });

  describe('Small Screen Behavior', () => {
    it('renders correctly at sm breakpoint (768-995px)', () => {
      setViewportSize(800, 600);
      render(<DashboardLayout />);

      const gridLayout = screen.getByTestId('grid-layout');
      expect(gridLayout).toBeInTheDocument();

      // Verify layout is passed correctly
      const layoutData = JSON.parse(gridLayout.getAttribute('data-layouts') || '{}');
      expect(layoutData.sm || layoutData.lg).toBeDefined();
    });

    it('falls back to mobile dashboard for xs breakpoint (480-767px)', () => {
      vi.mocked(useIsMobile).mockReturnValue(true);
      setViewportSize(500, 600);
      render(<DashboardLayout />);

      expect(screen.getByTestId('mobile-dashboard')).toBeInTheDocument();
    });

    it('falls back to mobile dashboard for xxs breakpoint (<480px)', () => {
      vi.mocked(useIsMobile).mockReturnValue(true);
      setViewportSize(400, 600);
      render(<DashboardLayout />);

      expect(screen.getByTestId('mobile-dashboard')).toBeInTheDocument();
    });
  });

  describe('Breakpoint inheritance', () => {
    it('omits breakpoints without saved layouts so they inherit from lg presets', () => {
      const { container } = render(<DashboardLayout />);
      const gridLayout = container.querySelector('[data-testid="grid-layout"]');
      expect(gridLayout).toBeInTheDocument();

      const layoutData = JSON.parse(gridLayout?.getAttribute('data-layouts') || '{}');
      expect(layoutData.lg).toBeDefined();
      expect(layoutData.md).toBeUndefined();
      expect(layoutData.sm).toBeUndefined();
      expect(layoutData.xs).toBeUndefined();
      expect(layoutData.xxs).toBeUndefined();
    });
  });

  describe('Layout Persistence Across Breakpoints', () => {
    it('maintains layout structure when viewport changes (desktop only)', () => {
      const { container, rerender } = render(<DashboardLayout />);

      // Initial render at large size
      let gridLayout = container.querySelector('[data-testid="grid-layout"]');
      let layoutData = JSON.parse(gridLayout?.getAttribute('data-layouts') || '{}');
      expect(layoutData.lg).toBeDefined();

      // Resize within desktop breakpoints (md)
      setViewportSize(1100, 600);
      rerender(<DashboardLayout />);

      gridLayout = container.querySelector('[data-testid="grid-layout"]');
      layoutData = JSON.parse(gridLayout?.getAttribute('data-layouts') || '{}');
      expect(layoutData.lg).toBeDefined();
    });
  });

  describe('Content Overflow Handling', () => {
    it('has overflow container on desktop layout', () => {
      setViewportSize(1024, 600);
      const { container } = render(<DashboardLayout />);

      const scrollContainer = container.querySelector('.flex-1.overflow-auto');
      expect(scrollContainer).toBeInTheDocument();

      // Verify overflow is enabled
      const styles = window.getComputedStyle(scrollContainer as Element);
      expect(['auto', 'scroll']).toContain(styles.overflowX);
    });
  });
});



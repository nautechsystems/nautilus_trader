// PanelWrapper tests

import { render, screen } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { PanelWrapper } from './PanelWrapper';

// Mock framer-motion
vi.mock('framer-motion', () => ({
  motion: {
    div: ({ children, ...props }: any) => <div {...props}>{children}</div>
  }
}));

// Mock PanelHeader
vi.mock('../shared/PanelHeader', () => ({
  PanelHeader: ({ title, className }: { title: string; className?: string }) => (
    <div data-testid="panel-header" className={className}>
      {title}
    </div>
  )
}));

describe('PanelWrapper', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders without crashing', () => {
    render(
      <PanelWrapper title="Test Panel">
        <div>Test Content</div>
      </PanelWrapper>
    );

    expect(screen.getByTestId('panel-header')).toBeInTheDocument();
    expect(screen.getByText('Test Content')).toBeInTheDocument();
  });

  it('renders panel body when not collapsed', () => {
    render(
      <PanelWrapper title="Test Panel">
        <div data-testid="panel-content">Test Content</div>
      </PanelWrapper>
    );

    expect(screen.getByTestId('panel-content')).toBeInTheDocument();
    expect(screen.getByTestId('panel-body')).toBeInTheDocument();
  });

  it('renders children inside panel body', () => {
    render(
      <PanelWrapper title="Test Panel">
        <div data-testid="child-content">Child Content</div>
      </PanelWrapper>
    );

    const panelBody = screen.getByTestId('panel-body');
    expect(panelBody).toBeInTheDocument();
    expect(screen.getByTestId('child-content')).toBeInTheDocument();
  });

  it('leaves scrolling responsibilities to children without injecting a nested container', () => {
    const { container } = render(
      <PanelWrapper title="Scrollable Panel">
        <div style={{ height: '2000px' }}>Tall Content</div>
      </PanelWrapper>
    );

    expect(container.querySelector('[data-testid="panel-scroll-container"]')).toBeNull();
    expect(screen.getByTestId('panel-body')).toHaveClass('overflow-hidden');
  });

  // Note: fullWidth is currently a layout hint passed from DashboardLayout
  // down into PanelWrapper and panels, but PanelWrapper itself does not
  // change its internal markup based on this flag anymore. The detailed
  // centering logic is now handled at the dashboard layout level.

  it('hydrates collapsed state from parent prop and updates when it changes', () => {
    const { rerender } = render(
      <PanelWrapper title="Collapsible Panel" collapsed>
        <div data-testid="collapsed-child">Hidden</div>
      </PanelWrapper>
    );

    expect(screen.queryByTestId('panel-body')).not.toBeInTheDocument();

    rerender(
      <PanelWrapper title="Collapsible Panel" collapsed={false}>
        <div data-testid="collapsed-child">Visible</div>
      </PanelWrapper>
    );

    expect(screen.getByTestId('panel-body')).toBeInTheDocument();
    expect(screen.getByTestId('collapsed-child')).toBeInTheDocument();
  });
});

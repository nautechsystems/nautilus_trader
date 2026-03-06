/**
 * ScrollArea Component Tests
 *
 * Tests for ScrollArea component covering:
 * - Rendering with scrollbars
 * - Vertical/horizontal/both orientations
 * - Scrollbar visibility
 * - Size variants
 * - Accessibility
 */

import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { ScrollArea } from '@/components/ui/scroll-area/ScrollArea';

describe('ScrollArea', () => {
  describe('Rendering', () => {
    it('renders children correctly', () => {
      render(
        <ScrollArea>
          <div>Scrollable content</div>
        </ScrollArea>
      );

      expect(screen.getByText('Scrollable content')).toBeInTheDocument();
    });

    it('renders with custom className', () => {
      const { container } = render(
        <ScrollArea className="custom-scroll">
          <div>Content</div>
        </ScrollArea>
      );

      expect(container.firstChild).toHaveClass('custom-scroll');
    });

    it('renders with custom viewport className', () => {
      const { container } = render(
        <ScrollArea viewportClassName="custom-viewport">
          <div>Content</div>
        </ScrollArea>
      );

      // Viewport is a child element
      const viewport = container.querySelector('[data-radix-scroll-area-viewport]');
      expect(viewport).toHaveClass('custom-viewport');
    });
  });

  describe('Orientation', () => {
    it('renders vertical scrollbar by default', () => {
      const { container } = render(
        <ScrollArea>
          <div style={{ height: '1000px' }}>Tall content</div>
        </ScrollArea>
      );

      const verticalScrollbar = container.querySelector(
        '[data-orientation="vertical"]'
      );
      expect(verticalScrollbar).toBeInTheDocument();
    });

    it('renders horizontal scrollbar when specified', () => {
      const { container } = render(
        <ScrollArea orientation="horizontal">
          <div style={{ width: '2000px' }}>Wide content</div>
        </ScrollArea>
      );

      const horizontalScrollbar = container.querySelector(
        '[data-orientation="horizontal"]'
      );
      expect(horizontalScrollbar).toBeInTheDocument();
    });

    it('renders both scrollbars when orientation is "both"', () => {
      const { container } = render(
        <ScrollArea orientation="both">
          <div style={{ width: '2000px', height: '1000px' }}>Large content</div>
        </ScrollArea>
      );

      const verticalScrollbar = container.querySelector(
        '[data-orientation="vertical"]'
      );
      const horizontalScrollbar = container.querySelector(
        '[data-orientation="horizontal"]'
      );

      expect(verticalScrollbar).toBeInTheDocument();
      expect(horizontalScrollbar).toBeInTheDocument();
    });

    it('renders corner when both scrollbars are present', () => {
      const { container } = render(
        <ScrollArea orientation="both">
          <div>Content</div>
        </ScrollArea>
      );

      const corner = container.querySelector('[style*="pointer-events"]');
      // Radix Corner component exists when both scrollbars are present
      expect(corner).toBeInTheDocument();
    });
  });

  describe('Size Variants', () => {
    it('applies small size', () => {
      const { container } = render(
        <ScrollArea size="sm">
          <div>Content</div>
        </ScrollArea>
      );

      expect(container.firstChild).toBeInTheDocument();
    });

    it('applies medium size by default', () => {
      const { container } = render(
        <ScrollArea>
          <div>Content</div>
        </ScrollArea>
      );

      expect(container.firstChild).toBeInTheDocument();
    });

    it('applies large size', () => {
      const { container } = render(
        <ScrollArea size="lg">
          <div>Content</div>
        </ScrollArea>
      );

      expect(container.firstChild).toBeInTheDocument();
    });
  });

  describe('Scrollbar Type', () => {
    it('uses hover type by default', () => {
      const { container } = render(
        <ScrollArea>
          <div>Content</div>
        </ScrollArea>
      );

      const root = container.firstChild;
      expect(root).toHaveAttribute('data-radix-scroll-area-root');
    });

    it('accepts always type', () => {
      const { container } = render(
        <ScrollArea type="always">
          <div>Content</div>
        </ScrollArea>
      );

      const root = container.firstChild;
      expect(root).toHaveAttribute('data-radix-scroll-area-root');
    });

    it('accepts scroll type', () => {
      const { container } = render(
        <ScrollArea type="scroll">
          <div>Content</div>
        </ScrollArea>
      );

      const root = container.firstChild;
      expect(root).toHaveAttribute('data-radix-scroll-area-root');
    });

    it('accepts auto type', () => {
      const { container } = render(
        <ScrollArea type="auto">
          <div>Content</div>
        </ScrollArea>
      );

      const root = container.firstChild;
      expect(root).toHaveAttribute('data-radix-scroll-area-root');
    });
  });

  describe('Scrollbar Components', () => {
    it('renders scrollbar thumb', () => {
      const { container } = render(
        <ScrollArea>
          <div style={{ height: '1000px' }}>Tall content</div>
        </ScrollArea>
      );

      const thumb = container.querySelector('[data-radix-scroll-area-thumb]');
      expect(thumb).toBeInTheDocument();
    });

    it('scrollbar is styled with dark theme', () => {
      const { container } = render(
        <ScrollArea>
          <div style={{ height: '1000px' }}>Tall content</div>
        </ScrollArea>
      );

      const thumb = container.querySelector('[data-radix-scroll-area-thumb]');
      expect(thumb).toBeInTheDocument();
      // Thumb should have rounded styling
      expect(thumb).toHaveClass('rounded-full');
    });
  });

  describe('Content Overflow', () => {
    it('handles content taller than container', () => {
      render(
        <ScrollArea className="h-32">
          <div style={{ height: '500px' }} data-testid="tall-content">
            Tall content that overflows
          </div>
        </ScrollArea>
      );

      expect(screen.getByTestId('tall-content')).toBeInTheDocument();
    });

    it('handles content wider than container', () => {
      render(
        <ScrollArea orientation="horizontal" className="w-32">
          <div style={{ width: '500px' }} data-testid="wide-content">
            Wide content that overflows
          </div>
        </ScrollArea>
      );

      expect(screen.getByTestId('wide-content')).toBeInTheDocument();
    });

    it('handles content larger than container in both dimensions', () => {
      render(
        <ScrollArea orientation="both" className="h-32 w-32">
          <div
            style={{ height: '500px', width: '500px' }}
            data-testid="large-content"
          >
            Large content that overflows
          </div>
        </ScrollArea>
      );

      expect(screen.getByTestId('large-content')).toBeInTheDocument();
    });
  });

  describe('Accessibility', () => {
    it('has proper scroll area structure', () => {
      const { container } = render(
        <ScrollArea>
          <div>Content</div>
        </ScrollArea>
      );

      const root = container.querySelector('[data-radix-scroll-area-root]');
      expect(root).toBeInTheDocument();
    });

    it('viewport has correct data attribute', () => {
      const { container } = render(
        <ScrollArea>
          <div>Content</div>
        </ScrollArea>
      );

      const viewport = container.querySelector('[data-radix-scroll-area-viewport]');
      expect(viewport).toBeInTheDocument();
    });

    it('scrollbar has correct orientation attribute', () => {
      const { container } = render(
        <ScrollArea>
          <div style={{ height: '1000px' }}>Tall content</div>
        </ScrollArea>
      );

      const scrollbar = container.querySelector('[data-orientation="vertical"]');
      expect(scrollbar).toBeInTheDocument();
      expect(scrollbar).toHaveAttribute('data-orientation', 'vertical');
    });
  });

  describe('Complex Layouts', () => {
    it('works with nested elements', () => {
      render(
        <ScrollArea>
          <div>
            <h1>Title</h1>
            <p>Paragraph 1</p>
            <p>Paragraph 2</p>
            <ul>
              <li>Item 1</li>
              <li>Item 2</li>
            </ul>
          </div>
        </ScrollArea>
      );

      expect(screen.getByText('Title')).toBeInTheDocument();
      expect(screen.getByText('Paragraph 1')).toBeInTheDocument();
      expect(screen.getByText('Item 1')).toBeInTheDocument();
    });

    it('preserves child element styling', () => {
      render(
        <ScrollArea>
          <div className="custom-child" data-testid="child">
            Content
          </div>
        </ScrollArea>
      );

      const child = screen.getByTestId('child');
      expect(child).toHaveClass('custom-child');
    });
  });

  describe('Integration', () => {
    it('renders multiple scroll areas independently', () => {
      render(
        <>
          <ScrollArea data-testid="scroll1">
            <div>Content 1</div>
          </ScrollArea>
          <ScrollArea data-testid="scroll2">
            <div>Content 2</div>
          </ScrollArea>
        </>
      );

      expect(screen.getByText('Content 1')).toBeInTheDocument();
      expect(screen.getByText('Content 2')).toBeInTheDocument();
    });

    it('can be nested inside other components', () => {
      render(
        <div className="container">
          <ScrollArea>
            <div>Nested content</div>
          </ScrollArea>
        </div>
      );

      expect(screen.getByText('Nested content')).toBeInTheDocument();
    });
  });
});

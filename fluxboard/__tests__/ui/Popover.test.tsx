/**
 * Popover Component Tests
 *
 * Tests for Popover component covering:
 * - Open/close behavior
 * - Positioning (side, align)
 * - Click outside to close
 * - Keyboard navigation (Esc to close)
 * - Arrow rendering
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Popover, PopoverClose, PopoverContentWrapper } from '@/components/ui/popover/Popover';

describe('Popover', () => {
  describe('Rendering', () => {
    it('renders trigger element', () => {
      render(
        <Popover trigger={<button>Open Popover</button>}>
          <div>Popover content</div>
        </Popover>
      );

      expect(screen.getByText('Open Popover')).toBeInTheDocument();
    });

    it('shows content when trigger is clicked', async () => {
      const user = userEvent.setup();

      render(
        <Popover trigger={<button>Open Popover</button>}>
          <div>Popover content</div>
        </Popover>
      );

      const trigger = screen.getByText('Open Popover');
      await user.click(trigger);

      await waitFor(() => {
        expect(screen.getByText('Popover content')).toBeInTheDocument();
      });
    });

    it('does not show content initially', () => {
      render(
        <Popover trigger={<button>Open Popover</button>}>
          <div>Popover content</div>
        </Popover>
      );

      expect(screen.queryByText('Popover content')).not.toBeInTheDocument();
    });

    it('renders arrow by default', async () => {
      const user = userEvent.setup();

      render(
        <Popover trigger={<button>Open Popover</button>}>
          <div>Popover content</div>
        </Popover>
      );

      await user.click(screen.getByText('Open Popover'));

      await waitFor(() => {
        const arrow = document.querySelector('svg[data-radix-popper-arrow]');
        expect(arrow).toBeInTheDocument();
      });
    });

    it('does not render arrow when showArrow is false', async () => {
      const user = userEvent.setup();

      render(
        <Popover trigger={<button>Open Popover</button>} showArrow={false}>
          <div>Popover content</div>
        </Popover>
      );

      await user.click(screen.getByText('Open Popover'));

      await waitFor(() => {
        expect(screen.getByText('Popover content')).toBeInTheDocument();
      });

      const arrow = document.querySelector('svg[data-radix-popper-arrow]');
      expect(arrow).not.toBeInTheDocument();
    });
  });

  describe('Open/Close Behavior', () => {
    it('opens popover when trigger is clicked', async () => {
      const user = userEvent.setup();

      render(
        <Popover trigger={<button>Open</button>}>
          <div>Content</div>
        </Popover>
      );

      await user.click(screen.getByText('Open'));

      await waitFor(() => {
        expect(screen.getByText('Content')).toBeInTheDocument();
      });
    });

    it('closes popover when clicking outside', async () => {
      const user = userEvent.setup();

      render(
        <div>
          <Popover trigger={<button>Open</button>}>
            <div>Content</div>
          </Popover>
          <div>Outside element</div>
        </div>
      );

      // Open popover
      await user.click(screen.getByText('Open'));
      await waitFor(() => {
        expect(screen.getByText('Content')).toBeInTheDocument();
      });

      // Click outside
      await user.click(screen.getByText('Outside element'));

      await waitFor(() => {
        expect(screen.queryByText('Content')).not.toBeInTheDocument();
      });
    });

    it('closes popover when Escape is pressed', async () => {
      const user = userEvent.setup();

      render(
        <Popover trigger={<button>Open</button>}>
          <div>Content</div>
        </Popover>
      );

      // Open popover
      await user.click(screen.getByText('Open'));
      await waitFor(() => {
        expect(screen.getByText('Content')).toBeInTheDocument();
      });

      // Press Escape
      await user.keyboard('{Escape}');

      await waitFor(() => {
        expect(screen.queryByText('Content')).not.toBeInTheDocument();
      });
    });

    it('closes popover when trigger is clicked again', async () => {
      const user = userEvent.setup();

      render(
        <Popover trigger={<button>Toggle</button>}>
          <div>Content</div>
        </Popover>
      );

      const trigger = screen.getByText('Toggle');

      // Open
      await user.click(trigger);
      await waitFor(() => {
        expect(screen.getByText('Content')).toBeInTheDocument();
      });

      // Close
      await user.click(trigger);
      await waitFor(() => {
        expect(screen.queryByText('Content')).not.toBeInTheDocument();
      });
    });
  });

  describe('Controlled Mode', () => {
    it('respects controlled open prop', async () => {
      const mockOnOpenChange = vi.fn();

      const { rerender } = render(
        <Popover
          trigger={<button>Open</button>}
          open={false}
          onOpenChange={mockOnOpenChange}
        >
          <div>Content</div>
        </Popover>
      );

      expect(screen.queryByText('Content')).not.toBeInTheDocument();

      // Change to open
      rerender(
        <Popover
          trigger={<button>Open</button>}
          open={true}
          onOpenChange={mockOnOpenChange}
        >
          <div>Content</div>
        </Popover>
      );

      await waitFor(() => {
        expect(screen.getByText('Content')).toBeInTheDocument();
      });
    });

    it('calls onOpenChange when trigger is clicked', async () => {
      const user = userEvent.setup();
      const mockOnOpenChange = vi.fn();

      render(
        <Popover
          trigger={<button>Open</button>}
          open={false}
          onOpenChange={mockOnOpenChange}
        >
          <div>Content</div>
        </Popover>
      );

      await user.click(screen.getByText('Open'));

      await waitFor(() => {
        expect(mockOnOpenChange).toHaveBeenCalledWith(true);
      });
    });
  });

  describe('Positioning', () => {
    it('accepts side prop without errors', async () => {
      const user = userEvent.setup();

      const sides: Array<'top' | 'right' | 'bottom' | 'left'> = ['top', 'right', 'bottom', 'left'];

      for (const side of sides) {
        const { unmount } = render(
          <Popover trigger={<button>Open {side}</button>} side={side}>
            <div>Content {side}</div>
          </Popover>
        );

        await user.click(screen.getByText(`Open ${side}`));
        await waitFor(() => {
          expect(screen.getByText(`Content ${side}`)).toBeInTheDocument();
        });

        unmount();
      }
    });

    it('accepts align prop without errors', async () => {
      const user = userEvent.setup();

      const aligns: Array<'start' | 'center' | 'end'> = ['start', 'center', 'end'];

      for (const align of aligns) {
        const { unmount } = render(
          <Popover trigger={<button>Open {align}</button>} align={align}>
            <div>Content {align}</div>
          </Popover>
        );

        await user.click(screen.getByText(`Open ${align}`));
        await waitFor(() => {
          expect(screen.getByText(`Content ${align}`)).toBeInTheDocument();
        });

        unmount();
      }
    });
  });

  describe('PopoverClose', () => {
    it('closes popover when PopoverClose is clicked', async () => {
      const user = userEvent.setup();

      render(
        <Popover trigger={<button>Open</button>}>
          <div>
            <p>Content</p>
            <PopoverClose asChild>
              <button>Close</button>
            </PopoverClose>
          </div>
        </Popover>
      );

      // Open popover
      await user.click(screen.getByText('Open'));
      await waitFor(() => {
        expect(screen.getByText('Content')).toBeInTheDocument();
      });

      // Click close button
      await user.click(screen.getByText('Close'));
      await waitFor(() => {
        expect(screen.queryByText('Content')).not.toBeInTheDocument();
      });
    });
  });

  describe('PopoverContentWrapper', () => {
    it('renders children correctly', async () => {
      const user = userEvent.setup();

      render(
        <Popover trigger={<button>Open</button>}>
          <PopoverContentWrapper>
            <p>Wrapped content</p>
          </PopoverContentWrapper>
        </Popover>
      );

      await user.click(screen.getByText('Open'));
      await waitFor(() => {
        expect(screen.getByText('Wrapped content')).toBeInTheDocument();
      });
    });

    it('applies padding sizes correctly', async () => {
      const user = userEvent.setup();

      const { rerender, unmount } = render(
        <Popover trigger={<button>Open</button>}>
          <PopoverContentWrapper padding="sm">
            <p>Small padding</p>
          </PopoverContentWrapper>
        </Popover>
      );

      await user.click(screen.getByText('Open'));
      await waitFor(() => {
        const wrapper = screen.getByText('Small padding').parentElement;
        expect(wrapper).toHaveClass('p-2');
      });

      unmount();

      render(
        <Popover trigger={<button>Open</button>}>
          <PopoverContentWrapper padding="lg">
            <p>Large padding</p>
          </PopoverContentWrapper>
        </Popover>
      );

      await user.click(screen.getByText('Open'));
      await waitFor(() => {
        const wrapper = screen.getByText('Large padding').parentElement;
        expect(wrapper).toHaveClass('p-4');
      });
    });
  });
});

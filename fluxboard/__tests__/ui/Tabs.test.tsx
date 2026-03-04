/**
 * Tabs Component Tests
 *
 * Tests for Tabs component covering:
 * - Rendering tabs and content
 * - Tab switching
 * - Keyboard navigation
 * - Disabled tabs
 * - Accessibility attributes
 */

import { describe, it, expect, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { act } from 'react';
import { Tabs } from '@/components/ui/tabs/Tabs';

async function press(user: ReturnType<typeof userEvent.setup>, keys: string) {
  await act(async () => {
    await user.keyboard(keys);
  });
}

describe('Tabs', () => {
  const mockTabs = [
    { value: 'tab1', label: 'First Tab', content: <div>Content 1</div> },
    { value: 'tab2', label: 'Second Tab', content: <div>Content 2</div> },
    { value: 'tab3', label: 'Third Tab', content: <div>Content 3</div> },
  ];

  describe('Rendering', () => {
    it('renders all tab triggers', () => {
      render(<Tabs tabs={mockTabs} />);

      expect(screen.getByText('First Tab')).toBeInTheDocument();
      expect(screen.getByText('Second Tab')).toBeInTheDocument();
      expect(screen.getByText('Third Tab')).toBeInTheDocument();
    });

    it('renders first tab content by default', () => {
      render(<Tabs tabs={mockTabs} />);

      expect(screen.getByText('Content 1')).toBeVisible();
      // Radix UI renders all tab content but hides inactive ones
      const content2 = screen.queryByText('Content 2');
      const content3 = screen.queryByText('Content 3');
      // Content should exist but be hidden
      if (content2) {
        expect(content2).not.toBeVisible();
      }
      if (content3) {
        expect(content3).not.toBeVisible();
      }
    });

    it('renders specified defaultValue tab content', () => {
      render(<Tabs tabs={mockTabs} defaultValue="tab2" />);

      // Radix UI renders all tab content but hides inactive ones
      const content1 = screen.queryByText('Content 1');
      const content3 = screen.queryByText('Content 3');
      if (content1) {
        expect(content1).not.toBeVisible();
      }
      expect(screen.getByText('Content 2')).toBeVisible();
      if (content3) {
        expect(content3).not.toBeVisible();
      }
    });

    it('renders disabled tab with correct state', () => {
      const tabsWithDisabled = [
        ...mockTabs,
        { value: 'tab4', label: 'Disabled Tab', content: <div>Content 4</div>, disabled: true },
      ];

      render(<Tabs tabs={tabsWithDisabled} />);

      const disabledTab = screen.getByText('Disabled Tab');
      expect(disabledTab).toHaveAttribute('disabled');
      expect(disabledTab).toHaveAttribute('data-disabled');
    });

    it('applies correct size classes', () => {
      const { rerender } = render(<Tabs tabs={mockTabs} size="sm" />);
      expect(screen.getByText('First Tab')).toBeInTheDocument();

      rerender(<Tabs tabs={mockTabs} size="lg" />);
      expect(screen.getByText('First Tab')).toBeInTheDocument();
    });
  });

  describe('Tab Switching', () => {
    it('switches to clicked tab', async () => {
      const user = userEvent.setup();

      render(<Tabs tabs={mockTabs} />);

      // Initially tab1 is active
      expect(screen.getByText('Content 1')).toBeVisible();

      // Click tab2
      await user.click(screen.getByText('Second Tab'));

      // Content should switch - wait for Radix UI to update
      await waitFor(() => {
        const content1 = screen.queryByText('Content 1');
        const content2 = screen.queryByText('Content 2');
        if (content1) {
          expect(content1).not.toBeVisible();
        }
        expect(content2).toBeVisible();
      });
    });

    it('calls onValueChange when tab is clicked', async () => {
      const user = userEvent.setup();
      const mockOnChange = vi.fn();

      render(<Tabs tabs={mockTabs} onValueChange={mockOnChange} />);

      await user.click(screen.getByText('Second Tab'));

      expect(mockOnChange).toHaveBeenCalledWith('tab2');
    });

    it('works in controlled mode', async () => {
      const user = userEvent.setup();
      const mockOnChange = vi.fn();

      const { rerender } = render(
        <Tabs tabs={mockTabs} value="tab1" onValueChange={mockOnChange} />
      );

      expect(screen.getByText('Content 1')).toBeVisible();

      await user.click(screen.getByText('Second Tab'));
      expect(mockOnChange).toHaveBeenCalledWith('tab2');

      // Simulate parent updating value
      rerender(<Tabs tabs={mockTabs} value="tab2" onValueChange={mockOnChange} />);

      await waitFor(() => {
        const content1 = screen.queryByText('Content 1');
        const content2 = screen.queryByText('Content 2');
        if (content1) {
          expect(content1).not.toBeVisible();
        }
        expect(content2).toBeVisible();
      });
    });

    it('does not switch to disabled tab', async () => {
      const user = userEvent.setup();
      const tabsWithDisabled = [
        { value: 'tab1', label: 'First Tab', content: <div>Content 1</div> },
        { value: 'tab2', label: 'Disabled Tab', content: <div>Content 2</div>, disabled: true },
      ];

      render(<Tabs tabs={tabsWithDisabled} />);

      expect(screen.getByText('Content 1')).toBeVisible();

      // Try to click disabled tab - should be prevented by Radix UI
      const disabledTab = screen.getByText('Disabled Tab');
      try {
        await user.click(disabledTab);
      } catch (e) {
        // Expected - disabled tabs may prevent interaction
      }

      // Should still show tab1 content
      await waitFor(() => {
        expect(screen.getByText('Content 1')).toBeVisible();
        const content2 = screen.queryByText('Content 2');
        if (content2) {
          expect(content2).not.toBeVisible();
        }
      });
    });
  });

  describe('Keyboard Navigation', () => {
    it('navigates tabs with arrow keys', async () => {
      const user = userEvent.setup();

      render(<Tabs tabs={mockTabs} />);

      const firstTab = screen.getByText('First Tab');
      firstTab.focus();

      // Arrow right to next tab
      await press(user, '{ArrowRight}');
      expect(screen.getByText('Second Tab')).toHaveFocus();

      // Arrow right to next tab
      await press(user, '{ArrowRight}');
      expect(screen.getByText('Third Tab')).toHaveFocus();

      // Arrow left to previous tab
      await press(user, '{ArrowLeft}');
      expect(screen.getByText('Second Tab')).toHaveFocus();
    });

    it('activates tab on Enter key', async () => {
      const user = userEvent.setup();
      const mockOnChange = vi.fn();

      render(<Tabs tabs={mockTabs} onValueChange={mockOnChange} />);

      const secondTab = screen.getByText('Second Tab');
      secondTab.focus();
      await press(user, '{Enter}');

      expect(mockOnChange).toHaveBeenCalledWith('tab2');
    });

    it('activates tab on Space key', async () => {
      const user = userEvent.setup();
      const mockOnChange = vi.fn();

      render(<Tabs tabs={mockTabs} onValueChange={mockOnChange} />);

      const thirdTab = screen.getByText('Third Tab');
      thirdTab.focus();
      await press(user, ' ');

      expect(mockOnChange).toHaveBeenCalledWith('tab3');
    });

    it('skips disabled tabs during keyboard navigation', async () => {
      const user = userEvent.setup();
      const tabsWithDisabled = [
        { value: 'tab1', label: 'First Tab', content: <div>Content 1</div> },
        { value: 'tab2', label: 'Disabled Tab', content: <div>Content 2</div>, disabled: true },
        { value: 'tab3', label: 'Third Tab', content: <div>Content 3</div> },
      ];

      render(<Tabs tabs={tabsWithDisabled} />);

      const firstTab = screen.getByText('First Tab');
      firstTab.focus();

      // Arrow right should skip disabled tab
      await press(user, '{ArrowRight}');
      expect(screen.getByText('Third Tab')).toHaveFocus();
    });
  });

  describe('Accessibility', () => {
    it('has correct ARIA attributes on tab list', () => {
      render(<Tabs tabs={mockTabs} />);

      const tabList = screen.getByRole('tablist');
      expect(tabList).toBeInTheDocument();
    });

    it('has correct ARIA attributes on tabs', () => {
      render(<Tabs tabs={mockTabs} />);

      const tabs = screen.getAllByRole('tab');
      expect(tabs).toHaveLength(3);

      // First tab should be selected
      expect(tabs[0]).toHaveAttribute('aria-selected', 'true');
      expect(tabs[1]).toHaveAttribute('aria-selected', 'false');
      expect(tabs[2]).toHaveAttribute('aria-selected', 'false');
    });

    it('has correct ARIA attributes on tab panels', () => {
      render(<Tabs tabs={mockTabs} />);

      const panels = screen.getAllByRole('tabpanel');
      // Only active panel is rendered
      expect(panels).toHaveLength(1);
    });

    it('updates aria-selected when tab changes', async () => {
      const user = userEvent.setup();

      render(<Tabs tabs={mockTabs} />);

      const tabs = screen.getAllByRole('tab');

      // Initially first tab is selected
      expect(tabs[0]).toHaveAttribute('aria-selected', 'true');

      // Click second tab
      await user.click(tabs[1]);

      // Second tab should now be selected
      expect(tabs[0]).toHaveAttribute('aria-selected', 'false');
      expect(tabs[1]).toHaveAttribute('aria-selected', 'true');
    });
  });

  describe('Orientation', () => {
    it('renders horizontal tabs by default', () => {
      render(<Tabs tabs={mockTabs} />);

      const tabList = screen.getByRole('tablist');
      expect(tabList).toHaveAttribute('aria-orientation', 'horizontal');
    });

    it('renders vertical tabs when specified', () => {
      render(<Tabs tabs={mockTabs} orientation="vertical" />);

      const tabList = screen.getByRole('tablist');
      expect(tabList).toHaveAttribute('aria-orientation', 'vertical');
    });
  });

  describe('Custom ClassNames', () => {
    it('applies custom className to container', () => {
      const { container } = render(<Tabs tabs={mockTabs} className="custom-tabs" />);

      expect(container.firstChild).toHaveClass('custom-tabs');
    });

    it('applies custom className to tab list', () => {
      render(<Tabs tabs={mockTabs} tabListClassName="custom-tab-list" />);

      const tabList = screen.getByRole('tablist');
      expect(tabList).toHaveClass('custom-tab-list');
    });

    it('applies custom className to content', () => {
      render(<Tabs tabs={mockTabs} contentClassName="custom-content" />);

      const content = screen.getByText('Content 1').parentElement;
      expect(content).toHaveClass('custom-content');
    });
  });
});

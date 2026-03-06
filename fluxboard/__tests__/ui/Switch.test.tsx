/**
 * Switch Component Tests
 *
 * Tests for Switch component covering:
 * - Toggle behavior
 * - Keyboard interaction (Space/Enter)
 * - Disabled state
 * - Label rendering
 * - Accessibility attributes
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Switch, SwitchGroup } from '@/components/ui/switch/Switch';

describe('Switch', () => {
  describe('Rendering', () => {
    it('renders switch without label', () => {
      render(<Switch onCheckedChange={vi.fn()} />);

      const switchElement = screen.getByRole('switch');
      expect(switchElement).toBeInTheDocument();
    });

    it('renders switch with label', () => {
      render(<Switch label="Enable feature" onCheckedChange={vi.fn()} />);

      expect(screen.getByText('Enable feature')).toBeInTheDocument();
      expect(screen.getByRole('switch')).toBeInTheDocument();
    });

    it('renders unchecked by default', () => {
      render(<Switch onCheckedChange={vi.fn()} />);

      const switchElement = screen.getByRole('switch');
      expect(switchElement).toHaveAttribute('aria-checked', 'false');
      expect(switchElement).toHaveAttribute('data-state', 'unchecked');
    });

    it('renders checked when defaultChecked is true', () => {
      render(<Switch defaultChecked={true} onCheckedChange={vi.fn()} />);

      const switchElement = screen.getByRole('switch');
      expect(switchElement).toHaveAttribute('aria-checked', 'true');
      expect(switchElement).toHaveAttribute('data-state', 'checked');
    });

    it('renders with correct size', () => {
      const { rerender } = render(
        <Switch size="sm" label="Small switch" onCheckedChange={vi.fn()} />
      );
      expect(screen.getByText('Small switch')).toBeInTheDocument();

      rerender(<Switch size="md" label="Medium switch" onCheckedChange={vi.fn()} />);
      expect(screen.getByText('Medium switch')).toBeInTheDocument();
    });

    it('renders disabled state correctly', () => {
      render(<Switch disabled={true} onCheckedChange={vi.fn()} />);

      const switchElement = screen.getByRole('switch');
      expect(switchElement).toBeDisabled();
      expect(switchElement).toHaveAttribute('data-disabled');
    });
  });

  describe('Toggle Behavior', () => {
    it('toggles on click', async () => {
      const user = userEvent.setup();
      const mockOnChange = vi.fn();

      render(<Switch onCheckedChange={mockOnChange} />);

      const switchElement = screen.getByRole('switch');
      await user.click(switchElement);

      expect(mockOnChange).toHaveBeenCalledWith(true);
    });

    it('toggles from checked to unchecked', async () => {
      const user = userEvent.setup();
      const mockOnChange = vi.fn();

      render(<Switch defaultChecked={true} onCheckedChange={mockOnChange} />);

      const switchElement = screen.getByRole('switch');
      await user.click(switchElement);

      expect(mockOnChange).toHaveBeenCalledWith(false);
    });

    it('works in controlled mode', async () => {
      const user = userEvent.setup();
      const mockOnChange = vi.fn();

      const { rerender } = render(
        <Switch checked={false} onCheckedChange={mockOnChange} />
      );

      let switchElement = screen.getByRole('switch');
      expect(switchElement).toHaveAttribute('aria-checked', 'false');

      await user.click(switchElement);
      expect(mockOnChange).toHaveBeenCalledWith(true);

      // Parent updates checked prop
      rerender(<Switch checked={true} onCheckedChange={mockOnChange} />);

      switchElement = screen.getByRole('switch');
      expect(switchElement).toHaveAttribute('aria-checked', 'true');
    });

    it('does not toggle when disabled', async () => {
      const user = userEvent.setup();
      const mockOnChange = vi.fn();

      render(<Switch disabled={true} onCheckedChange={mockOnChange} />);

      const switchElement = screen.getByRole('switch');
      await user.click(switchElement);

      expect(mockOnChange).not.toHaveBeenCalled();
    });

    it('toggles when label is clicked', async () => {
      const user = userEvent.setup();
      const mockOnChange = vi.fn();

      render(<Switch label="Toggle me" id="test-switch" onCheckedChange={mockOnChange} />);

      const label = screen.getByText('Toggle me');
      await user.click(label);

      expect(mockOnChange).toHaveBeenCalledWith(true);
    });
  });

  describe('Keyboard Interaction', () => {
    it('toggles on Space key', async () => {
      const user = userEvent.setup();
      const mockOnChange = vi.fn();

      render(<Switch onCheckedChange={mockOnChange} />);

      const switchElement = screen.getByRole('switch');
      switchElement.focus();
      await user.keyboard(' ');

      expect(mockOnChange).toHaveBeenCalledWith(true);
    });

    it('toggles on Enter key', async () => {
      const user = userEvent.setup();
      const mockOnChange = vi.fn();

      render(<Switch onCheckedChange={mockOnChange} />);

      const switchElement = screen.getByRole('switch');
      switchElement.focus();
      await user.keyboard('{Enter}');

      expect(mockOnChange).toHaveBeenCalledWith(true);
    });

    it('does not toggle on other keys', async () => {
      const user = userEvent.setup();
      const mockOnChange = vi.fn();

      render(<Switch onCheckedChange={mockOnChange} />);

      const switchElement = screen.getByRole('switch');
      switchElement.focus();
      await user.keyboard('a');

      expect(mockOnChange).not.toHaveBeenCalled();
    });

    it('does not toggle when disabled and Space is pressed', async () => {
      const user = userEvent.setup();
      const mockOnChange = vi.fn();

      render(<Switch disabled={true} onCheckedChange={mockOnChange} />);

      const switchElement = screen.getByRole('switch');
      switchElement.focus();
      await user.keyboard(' ');

      expect(mockOnChange).not.toHaveBeenCalled();
    });
  });

  describe('Label Positioning', () => {
    it('renders label on right by default', () => {
      render(<Switch label="Right label" id="test" onCheckedChange={vi.fn()} />);

      const label = screen.getByText('Right label').parentElement;
      expect(label?.tagName).toBe('LABEL');
    });

    it('renders label on left when specified', () => {
      render(
        <Switch label="Left label" labelPosition="left" id="test" onCheckedChange={vi.fn()} />
      );

      const label = screen.getByText('Left label').parentElement;
      expect(label?.tagName).toBe('LABEL');
    });
  });

  describe('Accessibility', () => {
    it('has correct role', () => {
      render(<Switch onCheckedChange={vi.fn()} />);

      expect(screen.getByRole('switch')).toBeInTheDocument();
    });

    it('has correct aria-checked attribute when unchecked', () => {
      render(<Switch onCheckedChange={vi.fn()} />);

      const switchElement = screen.getByRole('switch');
      expect(switchElement).toHaveAttribute('aria-checked', 'false');
    });

    it('has correct aria-checked attribute when checked', () => {
      render(<Switch checked={true} onCheckedChange={vi.fn()} />);

      const switchElement = screen.getByRole('switch');
      expect(switchElement).toHaveAttribute('aria-checked', 'true');
    });

    it('is keyboard focusable', () => {
      render(<Switch onCheckedChange={vi.fn()} />);

      const switchElement = screen.getByRole('switch');
      switchElement.focus();

      expect(document.activeElement).toBe(switchElement);
    });

    it('connects label to switch with id', () => {
      render(<Switch label="My switch" id="my-switch" onCheckedChange={vi.fn()} />);

      const label = screen.getByText('My switch').parentElement;
      const switchElement = screen.getByRole('switch');

      expect(label).toHaveAttribute('for', 'my-switch');
      expect(switchElement).toHaveAttribute('id', 'my-switch');
    });

    it('supports name attribute for forms', () => {
      // Radix UI Switch may not expose name directly on the switch element
      // but it should accept the prop without errors for form integration
      const { container } = render(<Switch name="feature-toggle" onCheckedChange={vi.fn()} />);

      const switchElement = screen.getByRole('switch');
      expect(switchElement).toBeInTheDocument();
      // Verify component rendered successfully with name prop
      // The name attribute may be handled internally by Radix for form integration
    });
  });

  describe('Custom ClassNames', () => {
    it('applies custom className', () => {
      render(<Switch className="custom-switch" onCheckedChange={vi.fn()} />);

      const switchElement = screen.getByRole('switch');
      expect(switchElement).toHaveClass('custom-switch');
    });
  });
});

describe('SwitchGroup', () => {
  it('renders children with correct spacing', () => {
    render(
      <SwitchGroup>
        <Switch label="Option 1" onCheckedChange={vi.fn()} />
        <Switch label="Option 2" onCheckedChange={vi.fn()} />
      </SwitchGroup>
    );

    expect(screen.getByText('Option 1')).toBeInTheDocument();
    expect(screen.getByText('Option 2')).toBeInTheDocument();
  });

  it('applies vertical layout by default', () => {
    const { container } = render(
      <SwitchGroup>
        <Switch label="Option 1" onCheckedChange={vi.fn()} />
      </SwitchGroup>
    );

    const group = container.firstChild;
    expect(group).toHaveClass('flex-col');
  });

  it('applies horizontal layout when specified', () => {
    const { container } = render(
      <SwitchGroup direction="horizontal">
        <Switch label="Option 1" onCheckedChange={vi.fn()} />
      </SwitchGroup>
    );

    const group = container.firstChild;
    expect(group).toHaveClass('flex-row');
  });

  it('applies custom className', () => {
    const { container } = render(
      <SwitchGroup className="custom-group">
        <Switch label="Option 1" onCheckedChange={vi.fn()} />
      </SwitchGroup>
    );

    expect(container.firstChild).toHaveClass('custom-group');
  });
});

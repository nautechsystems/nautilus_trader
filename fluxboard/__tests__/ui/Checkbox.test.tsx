/**
 * Checkbox Component Tests
 *
 * Tests for standard checkbox input with label and indeterminate state.
 *
 * Coverage:
 * - Toggle checked state
 * - Label rendering
 * - Indeterminate state
 * - Disabled state
 * - Dense mode
 * - Keyboard activation (Space)
 * - Accessibility attributes
 * - Custom className
 * - Ref forwarding
 *
 * Coverage target: >90%
 */

import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Checkbox } from '../../components/ui/input/Checkbox';

describe('Checkbox Component', () => {
  describe('Rendering', () => {
    it('renders with default props', () => {
      render(<Checkbox checked={false} onChange={vi.fn()} label="Test" />);
      expect(screen.getByLabelText('Test')).toBeInTheDocument();
    });

    it('renders checked checkbox', () => {
      render(<Checkbox checked={true} onChange={vi.fn()} label="Checked" />);
      const checkbox = screen.getByRole('checkbox', { name: 'Checked' });
      expect(checkbox).toBeChecked();
    });

    it('renders unchecked checkbox', () => {
      render(<Checkbox checked={false} onChange={vi.fn()} label="Unchecked" />);
      const checkbox = screen.getByRole('checkbox', { name: 'Unchecked' });
      expect(checkbox).not.toBeChecked();
    });

    it('renders without label', () => {
      render(
        <Checkbox checked={false} onChange={vi.fn()} aria-label="Unlabeled" />
      );
      expect(screen.getByLabelText('Unlabeled')).toBeInTheDocument();
    });

    it('applies custom className', () => {
      const { container } = render(
        <Checkbox
          checked={false}
          onChange={vi.fn()}
          label="Test"
          className="custom-checkbox"
        />
      );
      const label = container.querySelector('label');
      expect(label).toHaveClass('custom-checkbox');
    });

    it('forwards ref correctly', () => {
      const ref = vi.fn();
      render(<Checkbox ref={ref} checked={false} onChange={vi.fn()} label="Test" />);
      expect(ref).toHaveBeenCalled();
    });
  });

  describe('Toggle Behavior', () => {
    it('calls onChange when clicked', async () => {
      const handleChange = vi.fn();
      render(<Checkbox checked={false} onChange={handleChange} label="Toggle" />);

      const label = screen.getByText('Toggle');
      await userEvent.click(label);

      expect(handleChange).toHaveBeenCalledTimes(1);
      expect(handleChange).toHaveBeenCalledWith(true);
    });

    it('toggles from checked to unchecked', async () => {
      const handleChange = vi.fn();
      render(<Checkbox checked={true} onChange={handleChange} label="Toggle" />);

      const label = screen.getByText('Toggle');
      await userEvent.click(label);

      expect(handleChange).toHaveBeenCalledWith(false);
    });

    it('toggles from unchecked to checked', async () => {
      const handleChange = vi.fn();
      render(<Checkbox checked={false} onChange={handleChange} label="Toggle" />);

      const label = screen.getByText('Toggle');
      await userEvent.click(label);

      expect(handleChange).toHaveBeenCalledWith(true);
    });
  });

  describe('Indeterminate State', () => {
    it('renders indeterminate checkbox', () => {
      const { container } = render(
        <Checkbox
          checked={false}
          indeterminate={true}
          onChange={vi.fn()}
          label="Indeterminate"
        />
      );

      const checkbox = screen.getByRole('checkbox');
      expect(checkbox).toHaveProperty('indeterminate', true);
    });

    it('shows dash icon when indeterminate', () => {
      const { container } = render(
        <Checkbox
          checked={false}
          indeterminate={true}
          onChange={vi.fn()}
          label="Indeterminate"
        />
      );

      // Check for Minus icon (dash)
      const svg = container.querySelector('svg[class*="lucide-minus"]');
      expect(svg).toBeInTheDocument();
    });

    it('indeterminate overrides checked visual', () => {
      const { container } = render(
        <Checkbox
          checked={true}
          indeterminate={true}
          onChange={vi.fn()}
          label="Indeterminate"
        />
      );

      // Should show dash, not check
      const minusSvg = container.querySelector('svg[class*="lucide-minus"]');
      expect(minusSvg).toBeInTheDocument();
    });
  });

  describe('Disabled State', () => {
    it('renders disabled checkbox', () => {
      render(
        <Checkbox checked={false} onChange={vi.fn()} label="Disabled" disabled />
      );
      const checkbox = screen.getByRole('checkbox');
      expect(checkbox).toBeDisabled();
    });

    it('prevents onClick when disabled', async () => {
      const handleChange = vi.fn();
      render(
        <Checkbox
          checked={false}
          onChange={handleChange}
          label="Disabled"
          disabled
        />
      );

      const label = screen.getByText('Disabled');
      await userEvent.click(label);

      expect(handleChange).not.toHaveBeenCalled();
    });

    it('applies disabled styling', () => {
      const { container } = render(
        <Checkbox checked={false} onChange={vi.fn()} label="Disabled" disabled />
      );

      const label = container.querySelector('label');
      expect(label).toHaveClass('cursor-not-allowed');
    });

    it('has correct aria attributes when disabled', () => {
      render(
        <Checkbox checked={false} onChange={vi.fn()} label="Disabled" disabled />
      );
      const checkbox = screen.getByRole('checkbox');
      expect(checkbox).toBeDisabled();
    });
  });

  describe('Dense Mode', () => {
    it('applies dense styling', () => {
      const { container } = render(
        <Checkbox checked={false} onChange={vi.fn()} label="Dense" dense />
      );

      const label = container.querySelector('label');
      expect(label).toHaveClass('py-0.5');
    });

    it('uses smaller checkbox size in dense mode', () => {
      const { container } = render(
        <Checkbox checked={false} onChange={vi.fn()} label="Dense" dense />
      );

      const checkboxVisual = container.querySelector('span[class*="w-4"]');
      expect(checkboxVisual).toBeInTheDocument();
    });

    it('uses smaller text in dense mode', () => {
      const { container } = render(
        <Checkbox checked={false} onChange={vi.fn()} label="Dense" dense />
      );

      const labelText = container.querySelector('span[class*="text-xs"]');
      expect(labelText).toBeInTheDocument();
    });
  });

  describe('Keyboard Activation', () => {
    it('activates on Space key', () => {
      const handleChange = vi.fn();
      const { container } = render(
        <Checkbox checked={false} onChange={handleChange} label="Keyboard" />
      );

      const label = container.querySelector('label');
      fireEvent.keyDown(label!, { key: ' ' });

      expect(handleChange).toHaveBeenCalledTimes(1);
      expect(handleChange).toHaveBeenCalledWith(true);
    });

    it('toggles on Space from checked to unchecked', () => {
      const handleChange = vi.fn();
      const { container } = render(
        <Checkbox checked={true} onChange={handleChange} label="Keyboard" />
      );

      const label = container.querySelector('label');
      fireEvent.keyDown(label!, { key: ' ' });

      expect(handleChange).toHaveBeenCalledWith(false);
    });

    it('does not activate on other keys', () => {
      const handleChange = vi.fn();
      const { container } = render(
        <Checkbox checked={false} onChange={handleChange} label="Keyboard" />
      );

      const label = container.querySelector('label');
      fireEvent.keyDown(label!, { key: 'Enter' });
      fireEvent.keyDown(label!, { key: 'a' });

      expect(handleChange).not.toHaveBeenCalled();
    });

    it('prevents Space activation when disabled', () => {
      const handleChange = vi.fn();
      const { container } = render(
        <Checkbox
          checked={false}
          onChange={handleChange}
          label="Disabled"
          disabled
        />
      );

      const label = container.querySelector('label');
      fireEvent.keyDown(label!, { key: ' ' });

      expect(handleChange).not.toHaveBeenCalled();
    });
  });

  describe('Accessibility', () => {
    it('has checkbox role', () => {
      render(<Checkbox checked={false} onChange={vi.fn()} label="Accessible" />);
      expect(screen.getByRole('checkbox')).toBeInTheDocument();
    });

    it('uses label for accessible name', () => {
      render(<Checkbox checked={false} onChange={vi.fn()} label="My Checkbox" />);
      expect(screen.getByLabelText('My Checkbox')).toBeInTheDocument();
    });

    it('uses aria-label when no label prop', () => {
      render(
        <Checkbox
          checked={false}
          onChange={vi.fn()}
          aria-label="Unlabeled checkbox"
        />
      );
      expect(screen.getByLabelText('Unlabeled checkbox')).toBeInTheDocument();
    });

    it('is keyboard focusable', () => {
      const { container } = render(
        <Checkbox checked={false} onChange={vi.fn()} label="Focusable" />
      );

      const label = container.querySelector('label');
      expect(label).toHaveAttribute('tabIndex', '0');
    });

    it('is not focusable when disabled', () => {
      const { container } = render(
        <Checkbox checked={false} onChange={vi.fn()} label="Disabled" disabled />
      );

      const label = container.querySelector('label');
      expect(label).toHaveAttribute('tabIndex', '-1');
    });

    it('associates label with input via id', () => {
      render(
        <Checkbox checked={false} onChange={vi.fn()} label="Labeled" id="test-id" />
      );

      const checkbox = screen.getByRole('checkbox');
      expect(checkbox).toHaveAttribute('id', 'test-id');
    });
  });

  describe('Form Integration', () => {
    it('supports name attribute', () => {
      render(
        <Checkbox
          checked={false}
          onChange={vi.fn()}
          label="Form field"
          name="agreement"
        />
      );

      const checkbox = screen.getByRole('checkbox');
      expect(checkbox).toHaveAttribute('name', 'agreement');
    });

    it('reflects checked state in input', () => {
      render(<Checkbox checked={true} onChange={vi.fn()} label="Checked" />);
      const checkbox = screen.getByRole('checkbox');
      expect(checkbox).toBeChecked();
    });

    it('reflects unchecked state in input', () => {
      render(<Checkbox checked={false} onChange={vi.fn()} label="Unchecked" />);
      const checkbox = screen.getByRole('checkbox');
      expect(checkbox).not.toBeChecked();
    });
  });

  describe('Visual States', () => {
    it('shows check icon when checked', () => {
      const { container } = render(
        <Checkbox checked={true} onChange={vi.fn()} label="Checked" />
      );

      // Check icon is rendered
      const svg = container.querySelector('svg[class*="lucide-check"]');
      expect(svg).toBeInTheDocument();
    });

    it('does not show check icon when unchecked', () => {
      const { container } = render(
        <Checkbox checked={false} onChange={vi.fn()} label="Unchecked" />
      );

      // No check icon
      const svg = container.querySelector('svg[class*="lucide-check"]');
      expect(svg).not.toBeInTheDocument();
    });
  });

  describe('Combination States', () => {
    it('renders dense disabled checkbox', () => {
      const { container } = render(
        <Checkbox
          checked={false}
          onChange={vi.fn()}
          label="Dense Disabled"
          dense
          disabled
        />
      );

      const label = container.querySelector('label');
      expect(label).toHaveClass('py-0.5');
      expect(label).toHaveClass('cursor-not-allowed');
    });

    it('renders checked indeterminate checkbox', () => {
      render(
        <Checkbox
          checked={true}
          indeterminate={true}
          onChange={vi.fn()}
          label="Checked Indeterminate"
        />
      );

      const checkbox = screen.getByRole('checkbox');
      expect(checkbox).toBeChecked();
      expect(checkbox).toHaveProperty('indeterminate', true);
    });
  });
});

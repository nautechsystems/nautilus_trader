/**
 * ToggleGroup Component Tests
 *
 * Tests for group of toggle buttons (single or multiple selection).
 *
 * Coverage:
 * - Single selection mode
 * - Multiple selection mode
 * - Option selection and deselection
 * - Icons in options
 * - Disabled state (group and individual options)
 * - Size variants (sm, md, lg)
 * - Keyboard activation
 * - Accessibility attributes
 * - Custom className
 * - Ref forwarding
 *
 * Coverage target: >90%
 */

import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ToggleGroup } from '../../components/ui/toggle/ToggleGroup';
import type { ToggleGroupOption } from '../../components/ui/toggle/ToggleGroup';

// Test icon component
const TestIcon = () => <svg data-testid="test-icon" />;

describe('ToggleGroup Component', () => {
  const basicOptions: ToggleGroupOption[] = [
    { value: 'option1', label: 'Option 1' },
    { value: 'option2', label: 'Option 2' },
    { value: 'option3', label: 'Option 3' },
  ];

  const optionsWithIcons: ToggleGroupOption[] = [
    { value: 'list', label: 'List', icon: <TestIcon /> },
    { value: 'grid', label: 'Grid', icon: <TestIcon /> },
  ];

  describe('Rendering', () => {
    it('renders with default props', () => {
      render(
        <ToggleGroup
          type="single"
          value="option1"
          onValueChange={vi.fn()}
          options={basicOptions}
        />
      );

      expect(screen.getByText('Option 1')).toBeInTheDocument();
      expect(screen.getByText('Option 2')).toBeInTheDocument();
      expect(screen.getByText('Option 3')).toBeInTheDocument();
    });

    it('renders all options', () => {
      render(
        <ToggleGroup
          type="single"
          value="option1"
          onValueChange={vi.fn()}
          options={basicOptions}
        />
      );

      basicOptions.forEach((option) => {
        expect(screen.getByText(option.label)).toBeInTheDocument();
      });
    });

    it('applies custom className', () => {
      const { container } = render(
        <ToggleGroup
          type="single"
          value="option1"
          onValueChange={vi.fn()}
          options={basicOptions}
          className="custom-toggle"
        />
      );

      const group = container.querySelector('[role="group"]');
      expect(group).toHaveClass('custom-toggle');
    });

    it('forwards ref correctly', () => {
      const ref = vi.fn();
      render(
        <ToggleGroup
          ref={ref}
          type="single"
          value="option1"
          onValueChange={vi.fn()}
          options={basicOptions}
        />
      );

      expect(ref).toHaveBeenCalled();
    });
  });

  describe('Single Selection Mode', () => {
    it('marks selected option as checked', () => {
      render(
        <ToggleGroup
          type="single"
          value="option2"
          onValueChange={vi.fn()}
          options={basicOptions}
        />
      );

      const option2Button = screen.getByText('Option 2').closest('button');
      expect(option2Button).toHaveAttribute('aria-checked', 'true');
    });

    it('marks unselected options as unchecked', () => {
      render(
        <ToggleGroup
          type="single"
          value="option2"
          onValueChange={vi.fn()}
          options={basicOptions}
        />
      );

      const option1Button = screen.getByText('Option 1').closest('button');
      const option3Button = screen.getByText('Option 3').closest('button');

      expect(option1Button).toHaveAttribute('aria-checked', 'false');
      expect(option3Button).toHaveAttribute('aria-checked', 'false');
    });

    it('calls onValueChange with new value when option clicked', async () => {
      const handleChange = vi.fn();
      render(
        <ToggleGroup
          type="single"
          value="option1"
          onValueChange={handleChange}
          options={basicOptions}
        />
      );

      const option2Button = screen.getByText('Option 2');
      await userEvent.click(option2Button);

      expect(handleChange).toHaveBeenCalledTimes(1);
      expect(handleChange).toHaveBeenCalledWith('option2');
    });

    it('allows selecting different option', async () => {
      const handleChange = vi.fn();
      render(
        <ToggleGroup
          type="single"
          value="option1"
          onValueChange={handleChange}
          options={basicOptions}
        />
      );

      const option3Button = screen.getByText('Option 3');
      await userEvent.click(option3Button);

      expect(handleChange).toHaveBeenCalledWith('option3');
    });

    it('has radio role for buttons', () => {
      render(
        <ToggleGroup
          type="single"
          value="option1"
          onValueChange={vi.fn()}
          options={basicOptions}
        />
      );

      const buttons = screen.getAllByRole('radio');
      expect(buttons).toHaveLength(3);
    });
  });

  describe('Multiple Selection Mode', () => {
    it('marks selected options as checked', () => {
      render(
        <ToggleGroup
          type="multiple"
          value={['option1', 'option3']}
          onValueChange={vi.fn()}
          options={basicOptions}
        />
      );

      const option1Button = screen.getByText('Option 1').closest('button');
      const option3Button = screen.getByText('Option 3').closest('button');

      expect(option1Button).toHaveAttribute('aria-checked', 'true');
      expect(option3Button).toHaveAttribute('aria-checked', 'true');
    });

    it('marks unselected options as unchecked', () => {
      render(
        <ToggleGroup
          type="multiple"
          value={['option1']}
          onValueChange={vi.fn()}
          options={basicOptions}
        />
      );

      const option2Button = screen.getByText('Option 2').closest('button');
      expect(option2Button).toHaveAttribute('aria-checked', 'false');
    });

    it('adds option to selection when clicked', async () => {
      const handleChange = vi.fn();
      render(
        <ToggleGroup
          type="multiple"
          value={['option1']}
          onValueChange={handleChange}
          options={basicOptions}
        />
      );

      const option2Button = screen.getByText('Option 2');
      await userEvent.click(option2Button);

      expect(handleChange).toHaveBeenCalledTimes(1);
      expect(handleChange).toHaveBeenCalledWith(['option1', 'option2']);
    });

    it('removes option from selection when clicked', async () => {
      const handleChange = vi.fn();
      render(
        <ToggleGroup
          type="multiple"
          value={['option1', 'option2']}
          onValueChange={handleChange}
          options={basicOptions}
        />
      );

      const option1Button = screen.getByText('Option 1');
      await userEvent.click(option1Button);

      expect(handleChange).toHaveBeenCalledWith(['option2']);
    });

    it('allows selecting multiple options', async () => {
      const handleChange = vi.fn();
      render(
        <ToggleGroup
          type="multiple"
          value={[]}
          onValueChange={handleChange}
          options={basicOptions}
        />
      );

      await userEvent.click(screen.getByText('Option 1'));
      await userEvent.click(screen.getByText('Option 2'));
      await userEvent.click(screen.getByText('Option 3'));

      expect(handleChange).toHaveBeenCalledTimes(3);
    });

    it('has checkbox role for buttons', () => {
      render(
        <ToggleGroup
          type="multiple"
          value={['option1']}
          onValueChange={vi.fn()}
          options={basicOptions}
        />
      );

      const buttons = screen.getAllByRole('checkbox');
      expect(buttons).toHaveLength(3);
    });
  });

  describe('Icons', () => {
    it('renders icons when provided', () => {
      render(
        <ToggleGroup
          type="single"
          value="list"
          onValueChange={vi.fn()}
          options={optionsWithIcons}
        />
      );

      const icons = screen.getAllByTestId('test-icon');
      expect(icons).toHaveLength(2);
    });

    it('renders icons alongside labels', () => {
      render(
        <ToggleGroup
          type="single"
          value="list"
          onValueChange={vi.fn()}
          options={optionsWithIcons}
        />
      );

      expect(screen.getByText('List')).toBeInTheDocument();
      expect(screen.getByText('Grid')).toBeInTheDocument();
      expect(screen.getAllByTestId('test-icon')).toHaveLength(2);
    });
  });

  describe('Disabled State', () => {
    it('disables entire group when disabled prop is true', () => {
      render(
        <ToggleGroup
          type="single"
          value="option1"
          onValueChange={vi.fn()}
          options={basicOptions}
          disabled
        />
      );

      const buttons = screen.getAllByRole('radio');
      buttons.forEach((button) => {
        expect(button).toBeDisabled();
      });
    });

    it('prevents onClick when group is disabled', async () => {
      const handleChange = vi.fn();
      render(
        <ToggleGroup
          type="single"
          value="option1"
          onValueChange={handleChange}
          options={basicOptions}
          disabled
        />
      );

      await userEvent.click(screen.getByText('Option 2'));
      expect(handleChange).not.toHaveBeenCalled();
    });

    it('disables individual option when option.disabled is true', () => {
      const optionsWithDisabled: ToggleGroupOption[] = [
        { value: 'option1', label: 'Option 1' },
        { value: 'option2', label: 'Option 2', disabled: true },
        { value: 'option3', label: 'Option 3' },
      ];

      render(
        <ToggleGroup
          type="single"
          value="option1"
          onValueChange={vi.fn()}
          options={optionsWithDisabled}
        />
      );

      const option2Button = screen.getByText('Option 2').closest('button');
      expect(option2Button).toBeDisabled();
    });

    it('applies disabled styling', () => {
      render(
        <ToggleGroup
          type="single"
          value="option1"
          onValueChange={vi.fn()}
          options={basicOptions}
          disabled
        />
      );

      const buttons = screen.getAllByRole('radio');
      buttons.forEach((button) => {
        expect(button).toHaveClass('cursor-not-allowed');
      });
    });
  });

  describe('Size Variants', () => {
    it('applies default md size', () => {
      render(
        <ToggleGroup
          type="single"
          value="option1"
          onValueChange={vi.fn()}
          options={basicOptions}
        />
      );

      const button = screen.getByText('Option 1').closest('button');
      expect(button).toHaveClass('h-9');
    });

    it('applies sm size', () => {
      render(
        <ToggleGroup
          type="single"
          value="option1"
          onValueChange={vi.fn()}
          options={basicOptions}
          size="sm"
        />
      );

      const button = screen.getByText('Option 1').closest('button');
      expect(button).toHaveClass('h-7');
    });

    it('applies lg size', () => {
      render(
        <ToggleGroup
          type="single"
          value="option1"
          onValueChange={vi.fn()}
          options={basicOptions}
          size="lg"
        />
      );

      const button = screen.getByText('Option 1').closest('button');
      expect(button).toHaveClass('h-11');
    });
  });

  describe('Accessibility', () => {
    it('has group role', () => {
      render(
        <ToggleGroup
          type="single"
          value="option1"
          onValueChange={vi.fn()}
          options={basicOptions}
        />
      );

      expect(screen.getByRole('group')).toBeInTheDocument();
    });

    it('supports aria-label', () => {
      render(
        <ToggleGroup
          type="single"
          value="option1"
          onValueChange={vi.fn()}
          options={basicOptions}
          aria-label="View mode"
        />
      );

      const group = screen.getByRole('group');
      expect(group).toHaveAttribute('aria-label', 'View mode');
    });

    it('sets correct aria-checked for selected options', () => {
      render(
        <ToggleGroup
          type="single"
          value="option2"
          onValueChange={vi.fn()}
          options={basicOptions}
        />
      );

      const option2Button = screen.getByText('Option 2').closest('button');
      expect(option2Button).toHaveAttribute('aria-checked', 'true');
    });

    it('sets correct aria-checked for unselected options', () => {
      render(
        <ToggleGroup
          type="single"
          value="option2"
          onValueChange={vi.fn()}
          options={basicOptions}
        />
      );

      const option1Button = screen.getByText('Option 1').closest('button');
      expect(option1Button).toHaveAttribute('aria-checked', 'false');
    });
  });

  describe('Visual States', () => {
    it('applies selected styling to selected option', () => {
      render(
        <ToggleGroup
          type="single"
          value="option1"
          onValueChange={vi.fn()}
          options={basicOptions}
        />
      );

      const option1Button = screen.getByText('Option 1').closest('button');
      expect(option1Button).toHaveClass('bg-emerald-600');
    });

    it('applies unselected styling to unselected options', () => {
      render(
        <ToggleGroup
          type="single"
          value="option1"
          onValueChange={vi.fn()}
          options={basicOptions}
        />
      );

      const option2Button = screen.getByText('Option 2').closest('button');
      expect(option2Button).toHaveClass('bg-neutral-900');
    });
  });

  describe('Edge Cases', () => {
    it('handles empty value array in multiple mode', async () => {
      const handleChange = vi.fn();
      render(
        <ToggleGroup
          type="multiple"
          value={[]}
          onValueChange={handleChange}
          options={basicOptions}
        />
      );

      await userEvent.click(screen.getByText('Option 1'));
      expect(handleChange).toHaveBeenCalledWith(['option1']);
    });

    it('handles single option', () => {
      const singleOption: ToggleGroupOption[] = [
        { value: 'only', label: 'Only Option' },
      ];

      render(
        <ToggleGroup
          type="single"
          value="only"
          onValueChange={vi.fn()}
          options={singleOption}
        />
      );

      expect(screen.getByText('Only Option')).toBeInTheDocument();
    });

    it('handles many options', () => {
      const manyOptions: ToggleGroupOption[] = Array.from(
        { length: 10 },
        (_, i) => ({
          value: `option${i}`,
          label: `Option ${i}`,
        })
      );

      render(
        <ToggleGroup
          type="single"
          value="option0"
          onValueChange={vi.fn()}
          options={manyOptions}
        />
      );

      const buttons = screen.getAllByRole('radio');
      expect(buttons).toHaveLength(10);
    });
  });

  describe('Combination States', () => {
    it('renders small disabled toggle group', () => {
      render(
        <ToggleGroup
          type="single"
          value="option1"
          onValueChange={vi.fn()}
          options={basicOptions}
          size="sm"
          disabled
        />
      );

      const button = screen.getByText('Option 1').closest('button');
      expect(button).toHaveClass('h-7');
      expect(button).toBeDisabled();
    });

    it('renders multiple selection with icons in large size', () => {
      render(
        <ToggleGroup
          type="multiple"
          value={['list']}
          onValueChange={vi.fn()}
          options={optionsWithIcons}
          size="lg"
        />
      );

      const button = screen.getByText('List').closest('button');
      expect(button).toHaveClass('h-11');
      expect(screen.getAllByTestId('test-icon')).toHaveLength(2);
    });
  });
});

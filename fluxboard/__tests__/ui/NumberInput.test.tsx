/**
 * NumberInput Component Tests
 *
 * Tests for NumberInput component including validation, steppers, and precision.
 */

import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { NumberInput } from '@/components/ui/input/NumberInput';

describe('NumberInput', () => {
  describe('Rendering', () => {
    it('renders with numeric value', () => {
      render(
        <NumberInput
          value={42}
          onChange={vi.fn()}
        />
      );

      const input = screen.getByRole('textbox');
      expect(input).toHaveValue('42');
    });

    it('renders with empty value', () => {
      render(
        <NumberInput
          value=""
          onChange={vi.fn()}
        />
      );

      const input = screen.getByRole('textbox');
      expect(input).toHaveValue('');
    });

    it('renders with label', () => {
      render(
        <NumberInput
          value={0}
          onChange={vi.fn()}
          label="Quantity"
        />
      );

      expect(screen.getByText('Quantity')).toBeInTheDocument();
    });

    it('renders with hint', () => {
      render(
        <NumberInput
          value={0}
          onChange={vi.fn()}
          hint="Enter a number"
        />
      );

      expect(screen.getByText('Enter a number')).toBeInTheDocument();
    });

    it('renders increment/decrement buttons by default', () => {
      render(
        <NumberInput
          value={10}
          onChange={vi.fn()}
        />
      );

      expect(screen.getByLabelText('Increment')).toBeInTheDocument();
      expect(screen.getByLabelText('Decrement')).toBeInTheDocument();
    });

    it('hides steppers when showSteppers is false', () => {
      render(
        <NumberInput
          value={10}
          onChange={vi.fn()}
          showSteppers={false}
        />
      );

      expect(screen.queryByLabelText('Increment')).not.toBeInTheDocument();
      expect(screen.queryByLabelText('Decrement')).not.toBeInTheDocument();
    });
  });

  describe('Value Changes', () => {
    it('calls onChange with parsed number', () => {
      const handleChange = vi.fn();
      render(
        <NumberInput
          value=""
          onChange={handleChange}
        />
      );

      const input = screen.getByRole('textbox');
      fireEvent.change(input, { target: { value: '123' } });

      expect(handleChange).toHaveBeenCalledWith(123);
    });

    it('allows empty string', () => {
      const handleChange = vi.fn();
      render(
        <NumberInput
          value={42}
          onChange={handleChange}
        />
      );

      const input = screen.getByRole('textbox');
      fireEvent.change(input, { target: { value: '' } });

      expect(handleChange).toHaveBeenCalledWith('');
    });

    it('allows decimal values', () => {
      const handleChange = vi.fn();
      render(
        <NumberInput
          value=""
          onChange={handleChange}
        />
      );

      const input = screen.getByRole('textbox');
      fireEvent.change(input, { target: { value: '3.14' } });

      expect(handleChange).toHaveBeenCalledWith(3.14);
    });

    it('rejects non-numeric input', () => {
      const handleChange = vi.fn();
      render(
        <NumberInput
          value={10}
          onChange={handleChange}
        />
      );

      const input = screen.getByRole('textbox');
      fireEvent.change(input, { target: { value: 'abc' } });

      expect(handleChange).not.toHaveBeenCalled();
    });
  });

  describe('Min/Max Validation', () => {
    it('rejects value below min', () => {
      const handleChange = vi.fn();
      render(
        <NumberInput
          value={10}
          onChange={handleChange}
          min={0}
        />
      );

      const input = screen.getByRole('textbox');
      fireEvent.change(input, { target: { value: '-5' } });

      expect(handleChange).not.toHaveBeenCalled();
    });

    it('rejects value above max', () => {
      const handleChange = vi.fn();
      render(
        <NumberInput
          value={50}
          onChange={handleChange}
          max={100}
        />
      );

      const input = screen.getByRole('textbox');
      fireEvent.change(input, { target: { value: '150' } });

      expect(handleChange).not.toHaveBeenCalled();
    });

    it('accepts value within min/max range', () => {
      const handleChange = vi.fn();
      render(
        <NumberInput
          value={50}
          onChange={handleChange}
          min={0}
          max={100}
        />
      );

      const input = screen.getByRole('textbox');
      fireEvent.change(input, { target: { value: '75' } });

      expect(handleChange).toHaveBeenCalledWith(75);
    });

    it('disables increment button at max', () => {
      render(
        <NumberInput
          value={100}
          onChange={vi.fn()}
          max={100}
        />
      );

      const incrementBtn = screen.getByLabelText('Increment');
      expect(incrementBtn).toBeDisabled();
    });

    it('disables decrement button at min', () => {
      render(
        <NumberInput
          value={0}
          onChange={vi.fn()}
          min={0}
        />
      );

      const decrementBtn = screen.getByLabelText('Decrement');
      expect(decrementBtn).toBeDisabled();
    });
  });

  describe('Increment/Decrement Buttons', () => {
    it('increments value when increment button clicked', () => {
      const handleChange = vi.fn();
      render(
        <NumberInput
          value={10}
          onChange={handleChange}
          step={1}
        />
      );

      const incrementBtn = screen.getByLabelText('Increment');
      fireEvent.click(incrementBtn);

      expect(handleChange).toHaveBeenCalledWith(11);
    });

    it('decrements value when decrement button clicked', () => {
      const handleChange = vi.fn();
      render(
        <NumberInput
          value={10}
          onChange={handleChange}
          step={1}
        />
      );

      const decrementBtn = screen.getByLabelText('Decrement');
      fireEvent.click(decrementBtn);

      expect(handleChange).toHaveBeenCalledWith(9);
    });

    it('uses custom step value', () => {
      const handleChange = vi.fn();
      render(
        <NumberInput
          value={10}
          onChange={handleChange}
          step={5}
        />
      );

      const incrementBtn = screen.getByLabelText('Increment');
      fireEvent.click(incrementBtn);

      expect(handleChange).toHaveBeenCalledWith(15);
    });

    it('increments from empty value (treats as 0)', () => {
      const handleChange = vi.fn();
      render(
        <NumberInput
          value=""
          onChange={handleChange}
          step={1}
        />
      );

      const incrementBtn = screen.getByLabelText('Increment');
      fireEvent.click(incrementBtn);

      expect(handleChange).toHaveBeenCalledWith(1);
    });
  });

  describe('Keyboard Controls', () => {
    it('increments on ArrowUp key', () => {
      const handleChange = vi.fn();
      render(
        <NumberInput
          value={10}
          onChange={handleChange}
          step={1}
        />
      );

      const input = screen.getByRole('textbox');
      fireEvent.keyDown(input, { key: 'ArrowUp' });

      expect(handleChange).toHaveBeenCalledWith(11);
    });

    it('decrements on ArrowDown key', () => {
      const handleChange = vi.fn();
      render(
        <NumberInput
          value={10}
          onChange={handleChange}
          step={1}
        />
      );

      const input = screen.getByRole('textbox');
      fireEvent.keyDown(input, { key: 'ArrowDown' });

      expect(handleChange).toHaveBeenCalledWith(9);
    });

    it('respects max when using ArrowUp', () => {
      const handleChange = vi.fn();
      render(
        <NumberInput
          value={100}
          onChange={handleChange}
          max={100}
          step={1}
        />
      );

      const input = screen.getByRole('textbox');
      fireEvent.keyDown(input, { key: 'ArrowUp' });

      expect(handleChange).not.toHaveBeenCalled();
    });

    it('respects min when using ArrowDown', () => {
      const handleChange = vi.fn();
      render(
        <NumberInput
          value={0}
          onChange={handleChange}
          min={0}
          step={1}
        />
      );

      const input = screen.getByRole('textbox');
      fireEvent.keyDown(input, { key: 'ArrowDown' });

      expect(handleChange).not.toHaveBeenCalled();
    });
  });

  describe('Precision Formatting', () => {
    it('formats value on blur with specified precision', () => {
      const handleChange = vi.fn();
      render(
        <NumberInput
          value={3.14159}
          onChange={handleChange}
          precision={2}
        />
      );

      const input = screen.getByRole('textbox');
      fireEvent.blur(input);

      // Precision formatting rounds the value on blur
      expect(handleChange).toHaveBeenCalledWith(3.14);
    });

    it('does not format empty value on blur', () => {
      const handleChange = vi.fn();
      const handleBlur = vi.fn();
      render(
        <NumberInput
          value=""
          onChange={handleChange}
          onBlur={handleBlur}
          precision={2}
        />
      );

      const input = screen.getByRole('textbox');
      fireEvent.blur(input);

      expect(handleBlur).toHaveBeenCalled();
    });
  });

  describe('States', () => {
    it('applies disabled state', () => {
      render(
        <NumberInput
          value={10}
          onChange={vi.fn()}
          disabled
        />
      );

      const input = screen.getByRole('textbox');
      expect(input).toBeDisabled();
    });

    it('hides steppers when disabled', () => {
      render(
        <NumberInput
          value={10}
          onChange={vi.fn()}
          disabled
        />
      );

      expect(screen.queryByLabelText('Increment')).not.toBeInTheDocument();
      expect(screen.queryByLabelText('Decrement')).not.toBeInTheDocument();
    });

    it('shows error message', () => {
      render(
        <NumberInput
          value={10}
          onChange={vi.fn()}
          error="Invalid number"
        />
      );

      expect(screen.getByRole('alert')).toHaveTextContent('Invalid number');
    });
  });

  describe('Accessibility', () => {
    it('uses inputMode="decimal" for numeric input', () => {
      render(
        <NumberInput
          value={10}
          onChange={vi.fn()}
        />
      );

      const input = screen.getByRole('textbox');
      expect(input).toHaveAttribute('inputMode', 'decimal');
    });

    it('sets aria-invalid on error', () => {
      render(
        <NumberInput
          value={10}
          onChange={vi.fn()}
          error="Error"
        />
      );

      const input = screen.getByRole('textbox');
      expect(input).toHaveAttribute('aria-invalid', 'true');
    });
  });
});

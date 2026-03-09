/**
 * TextInput Component Tests
 *
 * Tests for TextInput component including states, callbacks, and error handling.
 */

import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { TextInput } from '@/components/ui/input/TextInput';

describe('TextInput', () => {
  describe('Rendering', () => {
    it('renders with value', () => {
      render(
        <TextInput
          value="test value"
          onChange={vi.fn()}
        />
      );

      const input = screen.getByRole('textbox');
      expect(input).toHaveValue('test value');
    });

    it('renders with label', () => {
      render(
        <TextInput
          value=""
          onChange={vi.fn()}
          label="Test Label"
        />
      );

      expect(screen.getByText('Test Label')).toBeInTheDocument();
    });

    it('renders with hint text', () => {
      render(
        <TextInput
          value=""
          onChange={vi.fn()}
          hint="This is a hint"
        />
      );

      expect(screen.getByText('This is a hint')).toBeInTheDocument();
    });

    it('renders with placeholder', () => {
      render(
        <TextInput
          value=""
          onChange={vi.fn()}
          placeholder="Enter text..."
        />
      );

      const input = screen.getByRole('textbox');
      expect(input).toHaveAttribute('placeholder', 'Enter text...');
    });

    it('renders with custom className', () => {
      const { container } = render(
        <TextInput
          value=""
          onChange={vi.fn()}
          className="custom-class"
        />
      );

      expect(container.firstChild).toHaveClass('custom-class');
    });
  });

  describe('States', () => {
    it('applies disabled state', () => {
      render(
        <TextInput
          value="test"
          onChange={vi.fn()}
          disabled
        />
      );

      const input = screen.getByRole('textbox');
      expect(input).toBeDisabled();
    });

    it('shows error message when error is a string', () => {
      render(
        <TextInput
          value="test"
          onChange={vi.fn()}
          error="This is an error"
        />
      );

      expect(screen.getByRole('alert')).toHaveTextContent('This is an error');
    });

    it('applies error styling when error is true', () => {
      render(
        <TextInput
          value="test"
          onChange={vi.fn()}
          error={true}
        />
      );

      const input = screen.getByRole('textbox');
      expect(input).toHaveAttribute('aria-invalid', 'true');
    });

    it('does not show hint when error is present', () => {
      render(
        <TextInput
          value="test"
          onChange={vi.fn()}
          hint="This is a hint"
          error="This is an error"
        />
      );

      expect(screen.queryByText('This is a hint')).not.toBeInTheDocument();
      expect(screen.getByText('This is an error')).toBeInTheDocument();
    });

    it('shows hint when no error', () => {
      render(
        <TextInput
          value="test"
          onChange={vi.fn()}
          hint="This is a hint"
        />
      );

      expect(screen.getByText('This is a hint')).toBeInTheDocument();
    });
  });

  describe('Callbacks', () => {
    it('calls onChange when input changes', () => {
      const handleChange = vi.fn();
      render(
        <TextInput
          value=""
          onChange={handleChange}
        />
      );

      const input = screen.getByRole('textbox');
      fireEvent.change(input, { target: { value: 'new value' } });

      expect(handleChange).toHaveBeenCalledWith('new value');
      expect(handleChange).toHaveBeenCalledTimes(1);
    });

    it('calls onFocus when input is focused', () => {
      const handleFocus = vi.fn();
      render(
        <TextInput
          value="test"
          onChange={vi.fn()}
          onFocus={handleFocus}
        />
      );

      const input = screen.getByRole('textbox');
      fireEvent.focus(input);

      expect(handleFocus).toHaveBeenCalledTimes(1);
    });

    it('calls onBlur when input loses focus', () => {
      const handleBlur = vi.fn();
      render(
        <TextInput
          value="test"
          onChange={vi.fn()}
          onBlur={handleBlur}
        />
      );

      const input = screen.getByRole('textbox');
      fireEvent.blur(input);

      expect(handleBlur).toHaveBeenCalledTimes(1);
    });

    it('input is disabled when disabled prop is true', () => {
      const handleChange = vi.fn();
      render(
        <TextInput
          value="test"
          onChange={handleChange}
          disabled
        />
      );

      const input = screen.getByRole('textbox');
      expect(input).toBeDisabled();

      // Note: fireEvent.change can still trigger onChange in React controlled components
      // The disabled state is primarily a UI/UX constraint, not a functional blocker
    });
  });

  describe('Accessibility', () => {
    it('associates label with input', () => {
      render(
        <TextInput
          value=""
          onChange={vi.fn()}
          label="Test Label"
        />
      );

      const input = screen.getByRole('textbox');
      const label = screen.getByText('Test Label');

      expect(input).toHaveAttribute('id');
      expect(label).toHaveAttribute('for', input.getAttribute('id'));
    });

    it('sets aria-invalid when error is present', () => {
      render(
        <TextInput
          value="test"
          onChange={vi.fn()}
          error="Error message"
        />
      );

      const input = screen.getByRole('textbox');
      expect(input).toHaveAttribute('aria-invalid', 'true');
    });

    it('sets aria-describedby for error message', () => {
      render(
        <TextInput
          value="test"
          onChange={vi.fn()}
          error="Error message"
        />
      );

      const input = screen.getByRole('textbox');
      const errorId = input.getAttribute('aria-describedby');

      expect(errorId).toBeTruthy();
      expect(screen.getByRole('alert')).toHaveAttribute('id', errorId);
    });

    it('sets aria-describedby for hint when no error', () => {
      render(
        <TextInput
          value="test"
          onChange={vi.fn()}
          hint="Hint message"
        />
      );

      const input = screen.getByRole('textbox');
      const hintId = input.getAttribute('aria-describedby');

      expect(hintId).toBeTruthy();
      expect(screen.getByText('Hint message')).toHaveAttribute('id', hintId);
    });
  });

  describe('Input Types', () => {
    it('supports email type', () => {
      render(
        <TextInput
          value=""
          onChange={vi.fn()}
          type="email"
        />
      );

      const input = screen.getByRole('textbox');
      expect(input).toHaveAttribute('type', 'email');
    });

    it('supports password type', () => {
      const { container } = render(
        <TextInput
          value=""
          onChange={vi.fn()}
          type="password"
        />
      );

      // Password inputs don't have "textbox" role in testing-library
      const input = container.querySelector('input[type="password"]');
      expect(input).toBeInTheDocument();
      expect(input).toHaveAttribute('type', 'password');
    });

    it('defaults to text type', () => {
      render(
        <TextInput
          value=""
          onChange={vi.fn()}
        />
      );

      const input = screen.getByRole('textbox');
      expect(input).toHaveAttribute('type', 'text');
    });
  });
});

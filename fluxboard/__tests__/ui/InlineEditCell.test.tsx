/**
 * InlineEditCell Component Tests
 *
 * Tests for InlineEditCell component including edit modes, keyboard controls, and validation.
 */

import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { InlineEditCell } from '@/components/ui/input/InlineEditCell';

describe('InlineEditCell', () => {
  describe('View Mode', () => {
    it('renders value in view mode', () => {
      render(
        <InlineEditCell
          value="test value"
          onChange={vi.fn()}
          onSave={vi.fn()}
        />
      );

      expect(screen.getByText('test value')).toBeInTheDocument();
    });

    it('renders numeric value in view mode', () => {
      render(
        <InlineEditCell
          value={42}
          onChange={vi.fn()}
          onSave={vi.fn()}
          type="number"
        />
      );

      expect(screen.getByText('42')).toBeInTheDocument();
    });

    it('enters edit mode on click', () => {
      render(
        <InlineEditCell
          value="test"
          onChange={vi.fn()}
          onSave={vi.fn()}
        />
      );

      const cell = screen.getByRole('button');
      fireEvent.click(cell);

      expect(screen.getByRole('textbox')).toBeInTheDocument();
    });

    it('enters edit mode on Enter key', () => {
      render(
        <InlineEditCell
          value="test"
          onChange={vi.fn()}
          onSave={vi.fn()}
        />
      );

      const cell = screen.getByRole('button');
      fireEvent.keyDown(cell, { key: 'Enter' });

      expect(screen.getByRole('textbox')).toBeInTheDocument();
    });

    it('enters edit mode on Space key', () => {
      render(
        <InlineEditCell
          value="test"
          onChange={vi.fn()}
          onSave={vi.fn()}
        />
      );

      const cell = screen.getByRole('button');
      fireEvent.keyDown(cell, { key: ' ' });

      expect(screen.getByRole('textbox')).toBeInTheDocument();
    });

    it('does not enter edit mode when disabled', () => {
      render(
        <InlineEditCell
          value="test"
          onChange={vi.fn()}
          onSave={vi.fn()}
          disabled
        />
      );

      const cell = screen.getByText('test');
      fireEvent.click(cell);

      expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
    });
  });

  describe('Edit Mode', () => {
    it('focuses input when entering edit mode', () => {
      render(
        <InlineEditCell
          value="test"
          onChange={vi.fn()}
          onSave={vi.fn()}
        />
      );

      const cell = screen.getByRole('button');
      fireEvent.click(cell);

      const input = screen.getByRole('textbox');
      expect(input).toHaveFocus();
    });

    it('shows input with current value', () => {
      render(
        <InlineEditCell
          value="test value"
          onChange={vi.fn()}
          onSave={vi.fn()}
        />
      );

      const cell = screen.getByRole('button');
      fireEvent.click(cell);

      const input = screen.getByRole('textbox');
      expect(input).toHaveValue('test value');
    });

    it('updates input value when typing', () => {
      const handleChange = vi.fn();
      render(
        <InlineEditCell
          value="test"
          onChange={handleChange}
          onSave={vi.fn()}
        />
      );

      fireEvent.click(screen.getByRole('button'));
      const input = screen.getByRole('textbox');
      fireEvent.change(input, { target: { value: 'new value' } });

      // Input should show the new value
      expect(input).toHaveValue('new value');
    });

    it('uses text-right alignment for number type', () => {
      render(
        <InlineEditCell
          value={42}
          onChange={vi.fn()}
          onSave={vi.fn()}
          type="number"
        />
      );

      fireEvent.click(screen.getByRole('button'));
      const input = screen.getByRole('textbox');

      expect(input).toHaveClass('text-right');
    });

    it('uses text-left alignment for text type', () => {
      render(
        <InlineEditCell
          value="test"
          onChange={vi.fn()}
          onSave={vi.fn()}
          type="text"
        />
      );

      fireEvent.click(screen.getByRole('button'));
      const input = screen.getByRole('textbox');

      expect(input).toHaveClass('text-left');
    });
  });

  describe('Saving', () => {
    it('saves value on Enter key', () => {
      const handleSave = vi.fn();
      render(
        <InlineEditCell
          value="test"
          onChange={vi.fn()}
          onSave={handleSave}
        />
      );

      fireEvent.click(screen.getByRole('button'));
      const input = screen.getByRole('textbox');
      fireEvent.change(input, { target: { value: 'new value' } });
      fireEvent.keyDown(input, { key: 'Enter' });

      expect(handleSave).toHaveBeenCalledWith('new value');
    });

    it('saves value on blur', () => {
      const handleSave = vi.fn();
      render(
        <InlineEditCell
          value="test"
          onChange={vi.fn()}
          onSave={handleSave}
        />
      );

      fireEvent.click(screen.getByRole('button'));
      const input = screen.getByRole('textbox');
      fireEvent.change(input, { target: { value: 'new value' } });
      fireEvent.blur(input);

      expect(handleSave).toHaveBeenCalledWith('new value');
    });

    it('exits edit mode after successful save', () => {
      render(
        <InlineEditCell
          value="test"
          onChange={vi.fn()}
          onSave={vi.fn()}
        />
      );

      fireEvent.click(screen.getByRole('button'));
      const input = screen.getByRole('textbox');
      fireEvent.change(input, { target: { value: 'new value' } });
      fireEvent.keyDown(input, { key: 'Enter' });

      expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
      expect(screen.getByRole('button')).toBeInTheDocument();
    });

    it('saves numeric value with precision formatting', () => {
      const handleSave = vi.fn();
      render(
        <InlineEditCell
          value={3.14159}
          onChange={vi.fn()}
          onSave={handleSave}
          type="number"
          precision={2}
        />
      );

      fireEvent.click(screen.getByRole('button'));
      const input = screen.getByRole('textbox');
      fireEvent.keyDown(input, { key: 'Enter' });

      expect(handleSave).toHaveBeenCalledWith(3.14);
    });
  });

  describe('Canceling', () => {
    it('reverts value on Escape key', () => {
      const handleCancel = vi.fn();
      render(
        <InlineEditCell
          value="original"
          onChange={vi.fn()}
          onSave={vi.fn()}
          onCancel={handleCancel}
        />
      );

      fireEvent.click(screen.getByRole('button'));
      const input = screen.getByRole('textbox');
      fireEvent.change(input, { target: { value: 'changed' } });
      fireEvent.keyDown(input, { key: 'Escape' });

      expect(handleCancel).toHaveBeenCalled();
      expect(screen.getByText('original')).toBeInTheDocument();
    });

    it('exits edit mode on Escape', () => {
      render(
        <InlineEditCell
          value="test"
          onChange={vi.fn()}
          onSave={vi.fn()}
        />
      );

      fireEvent.click(screen.getByRole('button'));
      const input = screen.getByRole('textbox');
      fireEvent.keyDown(input, { key: 'Escape' });

      expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
    });

    it('does not save when canceling', () => {
      const handleSave = vi.fn();
      render(
        <InlineEditCell
          value="original"
          onChange={vi.fn()}
          onSave={handleSave}
        />
      );

      fireEvent.click(screen.getByRole('button'));
      const input = screen.getByRole('textbox');
      fireEvent.change(input, { target: { value: 'changed' } });
      fireEvent.keyDown(input, { key: 'Escape' });

      expect(handleSave).not.toHaveBeenCalled();
    });
  });

  describe('Validation', () => {
    it('rejects empty value', () => {
      const handleSave = vi.fn();
      render(
        <InlineEditCell
          value="test"
          onChange={vi.fn()}
          onSave={handleSave}
        />
      );

      fireEvent.click(screen.getByRole('button'));
      const input = screen.getByRole('textbox');
      fireEvent.change(input, { target: { value: '' } });
      fireEvent.keyDown(input, { key: 'Enter' });

      expect(handleSave).not.toHaveBeenCalled();
      expect(input).toHaveAttribute('aria-invalid', 'true');
    });

    it('validates number type', () => {
      const handleSave = vi.fn();
      render(
        <InlineEditCell
          value={42}
          onChange={vi.fn()}
          onSave={handleSave}
          type="number"
        />
      );

      fireEvent.click(screen.getByRole('button'));
      const input = screen.getByRole('textbox');
      fireEvent.change(input, { target: { value: 'not a number' } });
      fireEvent.keyDown(input, { key: 'Enter' });

      expect(handleSave).not.toHaveBeenCalled();
    });

    it('validates min constraint for numbers', () => {
      const handleSave = vi.fn();
      render(
        <InlineEditCell
          value={10}
          onChange={vi.fn()}
          onSave={handleSave}
          type="number"
          min={0}
        />
      );

      fireEvent.click(screen.getByRole('button'));
      const input = screen.getByRole('textbox');
      fireEvent.change(input, { target: { value: '-5' } });
      fireEvent.keyDown(input, { key: 'Enter' });

      expect(handleSave).not.toHaveBeenCalled();
    });

    it('validates max constraint for numbers', () => {
      const handleSave = vi.fn();
      render(
        <InlineEditCell
          value={50}
          onChange={vi.fn()}
          onSave={handleSave}
          type="number"
          max={100}
        />
      );

      fireEvent.click(screen.getByRole('button'));
      const input = screen.getByRole('textbox');
      fireEvent.change(input, { target: { value: '150' } });
      fireEvent.keyDown(input, { key: 'Enter' });

      expect(handleSave).not.toHaveBeenCalled();
    });

    it('uses custom validation function', () => {
      const customValidation = (value: string | number) => {
        return typeof value === 'string' && value.startsWith('valid');
      };
      const handleSave = vi.fn();

      render(
        <InlineEditCell
          value="valid-test"
          onChange={vi.fn()}
          onSave={handleSave}
          validation={customValidation}
        />
      );

      fireEvent.click(screen.getByRole('button'));
      const input = screen.getByRole('textbox');

      // Valid input
      fireEvent.change(input, { target: { value: 'valid-new' } });
      fireEvent.keyDown(input, { key: 'Enter' });
      expect(handleSave).toHaveBeenCalledWith('valid-new');

      // Invalid input
      handleSave.mockClear();
      fireEvent.click(screen.getByRole('button'));
      fireEvent.change(screen.getByRole('textbox'), { target: { value: 'invalid' } });
      fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter' });
      expect(handleSave).not.toHaveBeenCalled();
    });

    it('stays in edit mode when validation fails', () => {
      render(
        <InlineEditCell
          value="test"
          onChange={vi.fn()}
          onSave={vi.fn()}
        />
      );

      fireEvent.click(screen.getByRole('button'));
      const input = screen.getByRole('textbox');
      fireEvent.change(input, { target: { value: '' } });
      fireEvent.keyDown(input, { key: 'Enter' });

      expect(screen.getByRole('textbox')).toBeInTheDocument();
    });

    it('shows error indicator when validation fails', () => {
      render(
        <InlineEditCell
          value="test"
          onChange={vi.fn()}
          onSave={vi.fn()}
        />
      );

      fireEvent.click(screen.getByRole('button'));
      const input = screen.getByRole('textbox');
      fireEvent.change(input, { target: { value: '' } });

      expect(input).toHaveAttribute('aria-invalid', 'true');
    });
  });

  describe('External Value Updates', () => {
    it('syncs with external value changes when not editing', () => {
      const { rerender } = render(
        <InlineEditCell
          value="initial"
          onChange={vi.fn()}
          onSave={vi.fn()}
        />
      );

      expect(screen.getByText('initial')).toBeInTheDocument();

      rerender(
        <InlineEditCell
          value="updated"
          onChange={vi.fn()}
          onSave={vi.fn()}
        />
      );

      expect(screen.getByText('updated')).toBeInTheDocument();
    });

    it('does not sync with external changes during edit', () => {
      const { rerender } = render(
        <InlineEditCell
          value="initial"
          onChange={vi.fn()}
          onSave={vi.fn()}
        />
      );

      fireEvent.click(screen.getByRole('button'));
      const input = screen.getByRole('textbox');
      fireEvent.change(input, { target: { value: 'editing' } });

      rerender(
        <InlineEditCell
          value="external update"
          onChange={vi.fn()}
          onSave={vi.fn()}
        />
      );

      expect(input).toHaveValue('editing');
    });
  });
});

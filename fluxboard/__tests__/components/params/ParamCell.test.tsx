/**
 * Unit tests for ParamCell component.
 */

import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import ParamCell from '../../../components/params/ParamCell';
import type { ParamDef } from '../../../types';

const mockBotOnParam: ParamDef = {
  key: 'bot_on',
  label: 'bot_on',
  description: 'Toggle strategy on/off',
  type: 'select',
  default: '0',
  options: [['1', 'On'], ['0', 'Off']],
  unit: null
};

const mockQtyParam: ParamDef = {
  key: 'qty',
  label: 'qty',
  description: 'Quantity',
  type: 'float',
  default: 1.0,
  min_value: 0.0001,
  max_value: 10000,
  step: 0.01,
  unit: 'base'
};

describe('ParamCell', () => {
  it('renders select dropdown for select type', () => {
    render(
      <ParamCell
        value="1"
        paramDef={mockBotOnParam}
        dirty={false}
        saving={false}
        onChange={() => {}}
      />
    );

    const select = screen.getByRole('combobox');
    expect(select).toBeInTheDocument();
    expect(select).toHaveValue('1');
  });

  it('renders text input for float type', () => {
    render(
      <ParamCell
        value="10.5"
        paramDef={mockQtyParam}
        dirty={false}
        saving={false}
        onChange={() => {}}
      />
    );

    const input = screen.getByRole('spinbutton');
    expect(input).toBeInTheDocument();
    expect(input).toHaveValue(10.5);
  });

  it('shows dirty indicator when dirty=true', () => {
    const { container } = render(
      <ParamCell
        value="10.5"
        paramDef={mockQtyParam}
        dirty={true}
        saving={false}
        onChange={() => {}}
      />
    );

    const input = container.querySelector('input');
    expect(input?.className).toContain('ring-amber-500/60');
  });

  it('shows error indicator when error present', () => {
    render(
      <ParamCell
        value="-10"
        paramDef={mockQtyParam}
        dirty={false}
        error="qty must be >= 0.0001"
        saving={false}
        onChange={() => {}}
      />
    );

    expect(screen.getByText('qty must be >= 0.0001')).toBeInTheDocument();
  });

  it('calls onChange when value changes', () => {
    const onChange = vi.fn();
    render(
      <ParamCell
        value="10"
        paramDef={mockQtyParam}
        dirty={false}
        saving={false}
        onChange={onChange}
      />
    );

    const input = screen.getByRole('spinbutton');
    fireEvent.change(input, { target: { value: '20' } });

    expect(onChange).toHaveBeenCalledWith('20');
  });

  it('calls onBlur when focus leaves', () => {
    const onBlur = vi.fn();
    render(
      <ParamCell
        value="10"
        paramDef={mockQtyParam}
        dirty={false}
        saving={false}
        onChange={() => {}}
        onBlur={onBlur}
      />
    );

    const input = screen.getByRole('spinbutton');
    fireEvent.blur(input);

    expect(onBlur).toHaveBeenCalled();
  });

  it('calls onSave on Enter key', () => {
    const onSave = vi.fn();
    render(
      <ParamCell
        value="10"
        paramDef={mockQtyParam}
        dirty={false}
        saving={false}
        onChange={() => {}}
        onSave={onSave}
      />
    );

    const input = screen.getByRole('spinbutton');
    fireEvent.keyDown(input, { key: 'Enter' });

    expect(onSave).toHaveBeenCalled();
  });

  it('reverts value on Escape key', () => {
    const onChange = vi.fn();
    render(
      <ParamCell
        value="10"
        paramDef={mockQtyParam}
        dirty={true}
        saving={false}
        onChange={onChange}
      />
    );

    const input = screen.getByRole('spinbutton');

    // Change value
    fireEvent.change(input, { target: { value: '20' } });
    expect(onChange).toHaveBeenCalledWith('20');

    // Press Escape - should revert to original
    fireEvent.keyDown(input, { key: 'Escape' });
    expect(onChange).toHaveBeenCalledWith('10');
  });

  it('disables input when saving', () => {
    render(
      <ParamCell
        value="10"
        paramDef={mockQtyParam}
        dirty={false}
        saving={true}
        onChange={() => {}}
      />
    );

    const input = screen.getByRole('spinbutton');
    expect(input).toBeDisabled();
  });

  it('shows loading spinner when saving', () => {
    const { container } = render(
      <ParamCell
        value="10"
        paramDef={mockQtyParam}
        dirty={false}
        saving={true}
        onChange={() => {}}
      />
    );

    const spinner = container.querySelector('.animate-spin');
    expect(spinner).toBeInTheDocument();
  });

  it('right-aligns numeric inputs', () => {
    const { container } = render(
      <ParamCell
        value="10"
        paramDef={mockQtyParam}
        dirty={false}
        saving={false}
        onChange={() => {}}
      />
    );

    const input = container.querySelector('input');
    expect(input).toHaveClass('text-right');
  });

  it('does not call onSave on Enter when error present', () => {
    const onSave = vi.fn();
    render(
      <ParamCell
        value="-10"
        paramDef={mockQtyParam}
        dirty={false}
        error="Invalid value"
        saving={false}
        onChange={() => {}}
        onSave={onSave}
      />
    );

    const input = screen.getByRole('spinbutton');
    fireEvent.keyDown(input, { key: 'Enter' });

    expect(onSave).not.toHaveBeenCalled();
  });

  it('has proper ARIA attributes', () => {
    render(
      <ParamCell
        value="10"
        paramDef={mockQtyParam}
        dirty={false}
        error="Error message"
        saving={false}
        onChange={() => {}}
      />
    );

    const input = screen.getByRole('spinbutton');
    expect(input).toHaveAttribute('aria-label', 'qty');
    expect(input).toHaveAttribute('aria-invalid', 'true');
    expect(input).toHaveAttribute('aria-describedby');
  });
});

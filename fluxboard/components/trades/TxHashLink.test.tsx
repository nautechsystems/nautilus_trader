import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { TxHashLink } from './TxHashLink';

describe('TxHashLink', () => {
  it('renders a clickable link when explorerUrl is provided', () => {
    const hash = '0xabcdef1234567890abcdef1234567890abcdef12';
    const href = `https://example.explorer/tx/${hash}`;
    render(<TxHashLink hash={hash} explorerUrl={href} />);

    const link = screen.getByRole('link');
    expect(link).toBeInTheDocument();
    expect(link).toHaveAttribute('href', href);
    // Should show short hash text
    expect(link.textContent).toMatch(/^0x[0-9a-f]{6}…[0-9a-f]{6}$/i);
  });

  it('renders non-clickable short hash when explorerUrl is missing', () => {
    const hash = '0xabcdef1234567890abcdef1234567890abcdef12';
    render(<TxHashLink hash={hash} />);

    // No anchor
    const links = screen.queryAllByRole('link');
    expect(links.length).toBe(0);

    // Renders short hash as text
    const text = screen.getByText(/0x[a-f0-9]{6}…[a-f0-9]{6}/i);
    expect(text).toBeInTheDocument();
  });

  it('renders em dash when hash is missing', () => {
    render(<TxHashLink />);
    expect(screen.getByText('—')).toBeInTheDocument();
  });
});


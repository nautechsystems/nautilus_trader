// App layout tests

import { render, waitFor } from '@testing-library/react';
import { BrowserRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, it, expect } from 'vitest';
import App from './App';
import { useSuiteStore } from './stores';

const DEFAULT_SUITE_STATE = {
  suite: 'all' as const,
};

beforeEach(() => {
  useSuiteStore.setState({ ...DEFAULT_SUITE_STATE });
});

afterEach(() => {
  useSuiteStore.setState({ ...DEFAULT_SUITE_STATE });
});

describe('App Layout', () => {
  it('renders with flexbox layout instead of calc', () => {
    const { container } = render(
      <BrowserRouter>
        <App profile="default" />
      </BrowserRouter>
    );

    const root = container.firstChild as HTMLElement;
    expect(root).toHaveClass('flex', 'flex-col', 'h-screen');
    expect(root).not.toHaveClass('w-full'); // Removed redundant w-full
  });

  it('renders nav and main with correct flex properties', () => {
    const { container } = render(
      <BrowserRouter>
        <App profile="default" />
      </BrowserRouter>
    );

    const main = container.querySelector('main');
    expect(main).toHaveClass('flex-1', 'overflow-hidden');

    // Verify no hardcoded height calc
    expect(main?.className).not.toContain('calc');
    expect(main?.className).not.toContain('40px');
  });

  it('maintains full viewport height', () => {
    const { container } = render(
      <BrowserRouter>
        <App profile="default" />
      </BrowserRouter>
    );

    const root = container.firstChild as HTMLElement;
    expect(root).toHaveClass('h-screen');
  });

  it('has proper overflow settings', () => {
    const { container } = render(
      <BrowserRouter>
        <App profile="default" />
      </BrowserRouter>
    );

    const root = container.firstChild as HTMLElement;
    const main = container.querySelector('main');

    expect(root).toHaveClass('overflow-hidden');
    expect(main).toHaveClass('overflow-hidden');
  });

  it('binds tokenmm profile to dex_arb suite', async () => {
    render(
      <BrowserRouter>
        <App profile="tokenmm" />
      </BrowserRouter>
    );

    await waitFor(() => {
      expect(useSuiteStore.getState().suite).toBe('dex_arb');
    });
  });

  it('binds equities profile to equities suite', async () => {
    render(
      <BrowserRouter>
        <App profile="equities" />
      </BrowserRouter>
    );

    await waitFor(() => {
      expect(useSuiteStore.getState().suite).toBe('equities');
    });
  });

  it('binds lp profile to dex_arb suite', async () => {
    render(
      <BrowserRouter>
        <App profile="lp" />
      </BrowserRouter>
    );

    await waitFor(() => {
      expect(useSuiteStore.getState().suite).toBe('dex_arb');
    });
  });

  it('binds default profile to all suite', async () => {
    render(
      <BrowserRouter>
        <App profile="default" />
      </BrowserRouter>
    );

    await waitFor(() => {
      expect(useSuiteStore.getState().suite).toBe('all');
    });
  });
});

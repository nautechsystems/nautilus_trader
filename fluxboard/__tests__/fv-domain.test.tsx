import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';

import { FVFormulaInspector } from '../components/domain/fv/FVFormulaInspector';
import { FVWhatMoved } from '../components/domain/fv/FVWhatMoved';

describe('FV domain components', () => {
  it('renders selected term formula text and latex', () => {
    render(
      <FVFormulaInspector
        terms={[
          {
            id: 3,
            name: 'CompTheo Mid HL10',
            trigger: 'onChMid',
            weight: 100,
            mode: 'power',
            beta: 0.5,
            value: 0.019,
            contribution: 0.019,
            contribution_delta: 0,
            formula_text: 'V = EMA1 * (Theo2/EMA2)^beta',
            formula_latex: 'V = EMA_1 \\\\cdot \\\\left(\\\\frac{Theo_2}{EMA_2}\\\\right)^{\\\\beta}',
          },
        ]}
        selectedTermId={3}
        onSelectTerm={vi.fn()}
      />
    );

    expect(screen.getByText('V = EMA1 * (Theo2/EMA2)^beta')).toBeInTheDocument();
    expect(screen.getByText(/source:/i)).toBeInTheDocument();
  });

  it('renders rich formula inspector content (katex + reconciliation)', () => {
    render(
      <FVFormulaInspector
        terms={[
          {
            id: 3,
            name: 'CompTheo Mid HL10',
            trigger: 'onChMid',
            weight: 100,
            mode: 'power',
            beta: 0.5,
            value: 0.019,
            contribution: 0.019,
            contribution_delta: 0,
            formula_text: 'V = EMA_1 * (Theo_2 / EMA_2)^beta',
            formula_latex: 'V = EMA_1 \\\\cdot \\\\left(\\\\frac{Theo_2}{EMA_2}\\\\right)^{\\\\beta}',
          },
        ]}
        selectedTermId={3}
        onSelectTerm={vi.fn()}
      />
    );

    expect(document.querySelector('.katex')).not.toBeNull();
    expect(screen.getByText(/Reconciliation/i)).toBeInTheDocument();
  });

  it('renders what moved summary for trade-triggered moves', () => {
    render(
      <FVWhatMoved
        whatMoved={{
          term_id: 4,
          term_name: 'CompTheo Trade HL10',
          trigger: 'trade',
          delta_contribution: 0.0123,
          side: 'buy',
          notional_usd: 5000,
        }}
      />
    );

    expect(screen.getByText(/CompTheo Trade HL10/)).toBeInTheDocument();
    expect(screen.getByText(/buy/i)).toBeInTheDocument();
  });

  it('renders overlay mover summary when overlay dominates', () => {
    render(
      <FVWhatMoved
        whatMoved={{
          kind: 'overlay',
          term_name: 'Signed Volume Overlay',
          trigger: 'timer',
          delta_overlay_pct: -0.0005,
          delta_final: -0.00001,
        }}
      />
    );

    expect(screen.getByText(/Signed Volume Overlay/)).toBeInTheDocument();
    expect(screen.getByText(/Δoverlay/i)).toBeInTheDocument();
  });

  it('renders explicit no-change state for kind=none', () => {
    render(
      <FVWhatMoved
        whatMoved={{
          kind: 'none',
          trigger: 'timer',
          delta_contribution: 0,
          delta_overlay_pct: 0,
          delta_final: 0,
        }}
      />
    );

    expect(screen.getByText(/No dominant mover/i)).toBeInTheDocument();
    expect(screen.getByText(/Δfinal=\s*\+0\.000000/)).toBeInTheDocument();
  });
});

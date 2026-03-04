/**
 * InlineEditCell Demo
 *
 * Example usage of InlineEditCell component in a table context.
 * Demonstrates inline editing for strategy parameters (similar to Params panel).
 */

import { useState } from 'react';
import { InlineEditCell } from '../components/ui';

// =============================================================================
// MOCK DATA
// =============================================================================

interface StrategyParam {
  id: string;
  key: string;
  value: string | number;
  type: 'text' | 'number';
  min?: number;
  max?: number;
  precision?: number;
}

const INITIAL_PARAMS: StrategyParam[] = [
  {
    id: '1',
    key: 'bot_on',
    value: '1',
    type: 'text',
  },
  {
    id: '2',
    key: 'qty',
    value: 50.0,
    type: 'number',
    min: 0,
    max: 1000,
    precision: 2,
  },
  {
    id: '3',
    key: 'edge_bps',
    value: 15.5,
    type: 'number',
    min: 0,
    max: 100,
    precision: 1,
  },
  {
    id: '4',
    key: 'max_position',
    value: 100,
    type: 'number',
    min: 0,
    max: 10000,
    precision: 0,
  },
  {
    id: '5',
    key: 'strategy_name',
    value: 'rooster_bybit_pusdplume',
    type: 'text',
  },
];

// =============================================================================
// DEMO COMPONENT
// =============================================================================

export default function InlineEditDemo() {
  const [params, setParams] = useState<StrategyParam[]>(INITIAL_PARAMS);
  const [lastAction, setLastAction] = useState<string>('');

  // Handle parameter change
  const handleChange = (id: string, newValue: string | number) => {
    setParams((prev) =>
      prev.map((param) =>
        param.id === id ? { ...param, value: newValue } : param
      )
    );
  };

  // Handle parameter save
  const handleSave = (id: string, newValue: string | number) => {
    const param = params.find((p) => p.id === id);
    setLastAction(`Saved ${param?.key}: ${newValue}`);
    console.log(`[SAVE] ${param?.key} = ${newValue}`);
  };

  // Handle cancel
  const handleCancel = (id: string) => {
    const param = params.find((p) => p.id === id);
    setLastAction(`Cancelled edit for ${param?.key}`);
    console.log(`[CANCEL] ${param?.key}`);
  };

  // Custom validator for bot_on (must be "0" or "1")
  const validateBotOn = (value: string | number): boolean => {
    const str = value.toString();
    return str === '0' || str === '1';
  };

  return (
    <div className="min-h-screen bg-[#0e0e10] p-8">
      <div className="max-w-4xl mx-auto">
        {/* Header */}
        <div className="mb-6">
          <h1 className="text-2xl font-semibold text-neutral-100 mb-2">
            InlineEditCell Demo
          </h1>
          <p className="text-sm text-neutral-400">
            Click any cell to edit. Press <kbd className="px-1.5 py-0.5 bg-neutral-800 border border-neutral-700 rounded text-xs font-mono">Enter</kbd> to save or <kbd className="px-1.5 py-0.5 bg-neutral-800 border border-neutral-700 rounded text-xs font-mono">Esc</kbd> to cancel.
          </p>
        </div>

        {/* Last Action Banner */}
        {lastAction && (
          <div className="mb-4 p-3 bg-emerald-900/20 border border-emerald-700/50 rounded text-sm text-emerald-400">
            <strong>Last Action:</strong> {lastAction}
          </div>
        )}

        {/* Params Table */}
        <div className="bg-[#151518] border border-neutral-700 rounded-lg overflow-hidden">
          {/* Table Header */}
          <div className="grid grid-cols-3 gap-4 px-4 py-3 bg-neutral-900 border-b border-neutral-700">
            <div className="text-xs font-semibold text-neutral-300 uppercase tracking-wide">
              Parameter Key
            </div>
            <div className="text-xs font-semibold text-neutral-300 uppercase tracking-wide">
              Value
            </div>
            <div className="text-xs font-semibold text-neutral-300 uppercase tracking-wide">
              Type / Constraints
            </div>
          </div>

          {/* Table Body */}
          <div className="divide-y divide-neutral-800">
            {params.map((param) => (
              <div
                key={param.id}
                className="grid grid-cols-3 gap-4 px-4 py-2 hover:bg-neutral-900/50 transition-colors"
              >
                {/* Key Column */}
                <div className="flex items-center">
                  <span className="text-sm font-mono text-neutral-200">
                    {param.key}
                  </span>
                </div>

                {/* Value Column (InlineEditCell) */}
                <div className="flex items-center">
                  <InlineEditCell
                    value={param.value}
                    onChange={(newValue) => handleChange(param.id, newValue)}
                    onSave={(newValue) => handleSave(param.id, newValue)}
                    onCancel={() => handleCancel(param.id)}
                    type={param.type}
                    min={param.min}
                    max={param.max}
                    precision={param.precision}
                    validation={
                      param.key === 'bot_on' ? validateBotOn : undefined
                    }
                    className="w-full"
                  />
                </div>

                {/* Constraints Column */}
                <div className="flex items-center">
                  <span className="text-xs text-neutral-400">
                    {param.type === 'number' ? (
                      <>
                        {param.min !== undefined && `min: ${param.min}`}
                        {param.max !== undefined &&
                          ` max: ${param.max}`}
                        {param.precision !== undefined &&
                          ` precision: ${param.precision}`}
                      </>
                    ) : param.key === 'bot_on' ? (
                      'must be "0" or "1"'
                    ) : (
                      'text'
                    )}
                  </span>
                </div>
              </div>
            ))}
          </div>
        </div>

        {/* Usage Instructions */}
        <div className="mt-8 p-4 bg-neutral-900 border border-neutral-700 rounded-lg">
          <h2 className="text-lg font-semibold text-neutral-100 mb-3">
            Usage Instructions
          </h2>
          <ul className="space-y-2 text-sm text-neutral-300">
            <li>
              <strong>Click</strong> any value cell to enter edit mode
            </li>
            <li>
              <strong>Enter</strong> - Save changes and exit edit mode
            </li>
            <li>
              <strong>Esc</strong> - Cancel changes and revert to original value
            </li>
            <li>
              <strong>Blur</strong> - Save changes when clicking outside (if valid)
            </li>
            <li>
              <strong>Validation</strong> - Red border indicates invalid input (prevents save)
            </li>
            <li>
              <strong>bot_on</strong> - Must be "0" or "1" (custom validation)
            </li>
            <li>
              <strong>qty</strong> - Number with min/max/precision constraints
            </li>
          </ul>
        </div>

        {/* Current State (Debug) */}
        <div className="mt-8 p-4 bg-neutral-900 border border-neutral-700 rounded-lg">
          <h2 className="text-lg font-semibold text-neutral-100 mb-3">
            Current State (Debug)
          </h2>
          <pre className="text-xs font-mono text-neutral-300 overflow-x-auto">
            {JSON.stringify(params, null, 2)}
          </pre>
        </div>
      </div>
    </div>
  );
}

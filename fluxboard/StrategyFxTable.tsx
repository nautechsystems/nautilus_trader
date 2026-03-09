// Strategy FX Configuration Table

import { useState } from 'react';
import type { StrategyFxConfig } from './types';
import { useMobileLayout } from '@/hooks/useMobileLayout';

type Props = {
  strategies: StrategyFxConfig[];
};

type SortColumn = keyof StrategyFxConfig;
type SortDirection = 'asc' | 'desc';

export default function StrategyFxTable({ strategies }: Props) {
  const [sortColumn, setSortColumn] = useState<SortColumn>('id');
  const [sortDirection, setSortDirection] = useState<SortDirection>('asc');
  const { isMobile } = useMobileLayout();

  const handleSort = (column: SortColumn) => {
    if (sortColumn === column) {
      setSortDirection(sortDirection === 'asc' ? 'desc' : 'asc');
    } else {
      setSortColumn(column);
      setSortDirection('asc');
    }
  };

  const sortedStrategies = [...strategies].sort((a, b) => {
    const aVal = a[sortColumn];
    const bVal = b[sortColumn];

    // Handle numeric values
    if (typeof aVal === 'number' && typeof bVal === 'number') {
      return sortDirection === 'asc' ? aVal - bVal : bVal - aVal;
    }

    // Handle string values
    const aStr = String(aVal || '').toLowerCase();
    const bStr = String(bVal || '').toLowerCase();
    const comparison = aStr.localeCompare(bStr);
    return sortDirection === 'asc' ? comparison : -comparison;
  });

  const SortIcon = ({ column }: { column: SortColumn }) => {
    if (sortColumn !== column) return null;
    return <span className="ml-1">{sortDirection === 'asc' ? '↑' : '↓'}</span>;
  };

  // Color coding for FX sources
  const sourceColor = (source: string) => {
    switch (source) {
      case 'service':
        return 'bg-emerald-500/10 text-emerald-400 border-emerald-500/20';
      case 'par':
        return 'bg-zinc-800 text-zinc-400 border-zinc-700';
      case 'constant':
        return 'bg-amber-500/10 text-amber-400 border-amber-500/20';
      case 'pool':
        return 'bg-blue-500/10 text-blue-400 border-blue-500/20';
      default:
        return 'bg-red-500/10 text-red-400 border-red-500/20';
    }
  };

  // Highlight spread values
  const spreadColor = (spread: number) => {
    if (spread === 0) return 'text-zinc-500';
    if (spread < 5) return 'text-amber-400';
    return 'text-emerald-400';
  };

  if (isMobile) {
    return (
      <div className="flex flex-col gap-3">
        {sortedStrategies.length === 0 ? (
          <div className="rounded border border-neutral-800 bg-neutral-900 p-3 text-center text-neutral-500 text-sm">
            No strategies found
          </div>
        ) : (
          sortedStrategies.map((strategy) => (
            <div
              key={strategy.id}
              className="rounded-2xl border border-neutral-800 bg-neutral-900 p-3 text-xs text-neutral-300"
            >
              <div className="flex items-center justify-between">
                <div className="flex flex-col">
                  <span className="font-mono text-sm text-neutral-100">{strategy.id}</span>
                  <span className="text-neutral-500">{strategy.name || 'Unnamed'}</span>
                </div>
                <span className={`px-2 py-0.5 rounded-full border ${sourceColor(strategy.fx_source)}`}>
                  {strategy.fx_source}
                </span>
              </div>
              <div className="mt-2 flex items-center justify-between">
                <span className="font-mono text-neutral-100">{strategy.fx_pair}</span>
                <span className={`font-mono ${spreadColor(strategy.estimated_spread_bps || 0)}`}>
                  {strategy.estimated_spread_bps || 0} bps
                </span>
              </div>
              <div className="mt-2 text-[11px] text-neutral-500">
                Gas Model: {strategy.gas_model || '—'}
              </div>
            </div>
          ))
        )}
      </div>
    );
  }

  return (
    <div className="overflow-x-auto rounded-lg border border-zinc-800">
      <table className="w-full text-xs">
        <thead className="bg-zinc-900 border-b border-zinc-800">
          <tr>
            <th
              onClick={() => handleSort('id')}
              className="px-3 py-2 text-left font-semibold text-zinc-400 cursor-pointer hover:text-zinc-200 transition-colors"
            >
              Strategy <SortIcon column="id" />
            </th>
            <th
              onClick={() => handleSort('name')}
              className="px-3 py-2 text-left font-semibold text-zinc-400 cursor-pointer hover:text-zinc-200 transition-colors"
            >
              Name <SortIcon column="name" />
            </th>
            <th
              onClick={() => handleSort('fx_pair')}
              className="px-3 py-2 text-left font-semibold text-zinc-400 cursor-pointer hover:text-zinc-200 transition-colors"
            >
              FX Pair <SortIcon column="fx_pair" />
            </th>
            <th
              onClick={() => handleSort('fx_source')}
              className="px-3 py-2 text-left font-semibold text-zinc-400 cursor-pointer hover:text-zinc-200 transition-colors"
            >
              Source <SortIcon column="fx_source" />
            </th>
            <th
              onClick={() => handleSort('gas_model')}
              className="px-3 py-2 text-left font-semibold text-zinc-400 cursor-pointer hover:text-zinc-200 transition-colors"
            >
              Gas Model <SortIcon column="gas_model" />
            </th>
            <th
              onClick={() => handleSort('estimated_spread_bps')}
              className="px-3 py-2 text-right font-semibold text-zinc-400 cursor-pointer hover:text-zinc-200 transition-colors"
            >
              Est. Spread (bps) <SortIcon column="estimated_spread_bps" />
            </th>
          </tr>
        </thead>
        <tbody className="bg-zinc-950">
          {sortedStrategies.length === 0 ? (
            <tr>
              <td colSpan={6} className="px-3 py-8 text-center text-zinc-500">
                No strategies found
              </td>
            </tr>
          ) : (
            sortedStrategies.map((strategy, idx) => (
              <tr
                key={strategy.id}
                className={`border-b border-zinc-800 hover:bg-zinc-900/50 transition-colors ${
                  idx % 2 === 1 ? 'bg-zinc-900/30' : ''
                }`}
              >
                <td className="px-3 py-2 font-mono text-zinc-300">{strategy.id}</td>
                <td className="px-3 py-2 font-medium text-zinc-300">{strategy.name}</td>
                <td className="px-3 py-2 font-mono text-zinc-400">{strategy.fx_pair}</td>
                <td className="px-3 py-2">
                  <span
                    className={`inline-block px-2 py-0.5 text-[10px] font-mono rounded border uppercase tracking-wide ${sourceColor(
                      strategy.fx_source
                    )}`}
                  >
                    {strategy.fx_source}
                  </span>
                </td>
                <td className="px-3 py-2 font-mono text-zinc-500">
                  {strategy.gas_model}
                </td>
                <td
                  className={`px-3 py-2 text-right font-mono ${spreadColor(
                    strategy.estimated_spread_bps
                  )}`}
                >
                  {strategy.estimated_spread_bps}
                </td>
              </tr>
            ))
          )}
        </tbody>
      </table>
    </div>
  );
}

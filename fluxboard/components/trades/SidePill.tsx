// Side cell - plain text with color coding

export const SideCell = ({ v }: { v: 'buy' | 'sell' | string }) => {
  const isBuy = v?.toLowerCase() === 'buy';
  const isSell = v?.toLowerCase() === 'sell';

  // Color code buy/sell
  let colorClass = 'text-zinc-400';
  if (isBuy) colorClass = 'text-emerald-400';
  if (isSell) colorClass = 'text-red-400';

  return (
    <span className={`text-xs font-medium uppercase tracking-tight ${colorClass}`}>
      {v || '—'}
    </span>
  );
};

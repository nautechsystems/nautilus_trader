// Parameter row component for strategy parameters table

import { memo } from 'react';

export type ParamDef = {
  key: string;
  label: string;
  inputType?: 'select' | 'text';
  options?: { value: string; label: string }[];
  numeric?: boolean;
};

export type StrategyData = {
  id: string;
  hot_params?: string[];
  parameters?: Record<string, string>;
  status?: {
    runner_job?: string;
    running?: boolean | null;
    metrics_available?: boolean;
    shard?: string;
  };
};

type ParamRowProps = {
  strategy: StrategyData;
  paramValues: Record<string, string>;
  dirtyParams: Set<string>;
  saving: boolean;
  paramMapping: ParamDef[];
  onParamChange: (paramKey: string, value: string) => void;
  onSave: () => void;
};

const ParamRow = memo(({
  strategy,
  paramValues,
  dirtyParams,
  saving,
  paramMapping,
  onParamChange,
  onSave
}: ParamRowProps) => {
  const isDirty = dirtyParams.size > 0;

  const renderStatusBadge = (status?: StrategyData['status']) => {
    if (!status || !status.runner_job) {
      return <span className="px-2 py-1 rounded bg-yellow-700 text-xs">Unassigned</span>;
    }

    const { running, metrics_available, runner_job, shard } = status;

    return (
      <div className="flex flex-wrap gap-1 items-center text-xs">
        <span className="px-2 py-1 rounded bg-blue-600">{runner_job}</span>
        {metrics_available === false ? (
          <span className="px-2 py-1 rounded bg-yellow-700">Metrics offline</span>
        ) : running === true ? (
          <span className="px-2 py-1 rounded bg-green-600">Running</span>
        ) : running === false ? (
          <span className="px-2 py-1 rounded bg-red-600">Stopped</span>
        ) : (
          <span className="px-2 py-1 rounded bg-neutral-600">Unknown</span>
        )}
        {shard && <span className="px-2 py-1 rounded bg-neutral-700">{shard}</span>}
      </div>
    );
  };

  return (
    <tr className="odd:bg-neutral-900">
      <td className="p-2 sticky left-0 bg-neutral-950 odd:bg-neutral-900">
        <strong className="text-xs">{strategy.id}</strong>
      </td>
      <td className="p-2 sticky left-[180px] bg-neutral-950 odd:bg-neutral-900">
        {renderStatusBadge(strategy.status)}
      </td>
      <td className="p-2 sticky left-[320px] bg-neutral-950 odd:bg-neutral-900">
        <button
          onClick={onSave}
          disabled={!isDirty || saving}
          className={`px-3 py-1 rounded text-xs ${
            isDirty && !saving
              ? 'bg-blue-600 hover:bg-blue-700 cursor-pointer'
              : 'bg-neutral-700 cursor-not-allowed opacity-50'
          }`}
        >
          {saving ? 'Saving...' : 'Save'}
        </button>
      </td>
      {paramMapping.map(param => {
        const value = paramValues[param.key] || '';
        const dirty = dirtyParams.has(param.key);

        return (
          <td
            key={param.key}
            className={`p-2 ${dirty ? 'bg-yellow-900/30' : ''} ${param.numeric ? 'text-right' : 'text-center'}`}
          >
            {param.inputType === 'select' && param.options ? (
              <select
                value={value}
                onChange={(e) => onParamChange(param.key, e.target.value)}
                disabled={saving}
                className="w-full bg-neutral-900 border border-neutral-700 rounded px-2 py-1 text-xs"
              >
                <option value="">—</option>
                {param.options.map(opt => (
                  <option key={opt.value} value={opt.value}>
                    {opt.label}
                  </option>
                ))}
              </select>
            ) : (
              <input
                type="text"
                value={value}
                onChange={(e) => onParamChange(param.key, e.target.value)}
                disabled={saving}
                placeholder={param.label}
                className="w-full bg-neutral-900 border border-neutral-700 rounded px-2 py-1 text-xs"
              />
            )}
          </td>
        );
      })}
    </tr>
  );
});

ParamRow.displayName = 'ParamRow';

export default ParamRow;

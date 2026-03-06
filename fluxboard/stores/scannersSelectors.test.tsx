import React from 'react';
import { act, cleanup, render } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { shallow } from 'zustand/shallow';
import {
  selectScannersTableData,
  selectScannersTableTelemetry,
  useScannersStore,
} from './scannersStore';

function TelemetryProbe({ onRender }: { onRender: (count: number) => void }) {
  const slice = useScannersStore(selectScannersTableTelemetry, shallow);
  const countRef = React.useRef(0);
  countRef.current += 1;
  onRender(countRef.current);
  return (
    <div data-testid="telemetry-probe">
      {slice.lastAppliedAtTs}:{slice.updatesPerSec}:{slice.applyDurationP95Ms}:{slice.deltaBufferSize}
    </div>
  );
}

function TableDataProbe({ onRender }: { onRender: (count: number) => void }) {
  const slice = useScannersStore(selectScannersTableData, shallow);
  const countRef = React.useRef(0);
  countRef.current += 1;
  onRender(countRef.current);
  return (
    <div data-testid="table-data-probe">
      {slice.filteredIds.length}:{slice.loading ? '1' : '0'}:{slice.refreshing ? '1' : '0'}
    </div>
  );
}

describe('scannersStore selectors (granularity)', () => {
  beforeEach(() => {
    useScannersStore.setState((state) => ({
      loading: false,
      refreshing: false,
      hasMore: false,
      filteredIds: [],
      stats: {
        ...state.stats,
        lastAppliedAtTs: 0,
        updatesPerSec: 0,
        applyDurationP95Ms: 0,
        deltaBufferSize: 0,
        renderDurationP50Ms: 0,
      },
    }));
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('does not rerender telemetry selector when unrelated stats fields change', () => {
    const onRender = vi.fn();
    render(<TelemetryProbe onRender={onRender} />);

    expect(onRender).toHaveBeenCalledTimes(1);

    act(() => {
      useScannersStore.setState((state) => ({
        stats: {
          ...state.stats,
          renderDurationP50Ms: state.stats.renderDurationP50Ms + 1,
        },
      }));
    });

    expect(onRender).toHaveBeenCalledTimes(1);
  });

  it('rerenders telemetry selector when selected stats fields change', () => {
    const onRender = vi.fn();
    render(<TelemetryProbe onRender={onRender} />);

    expect(onRender).toHaveBeenCalledTimes(1);

    act(() => {
      useScannersStore.setState((state) => ({
        stats: {
          ...state.stats,
          deltaBufferSize: state.stats.deltaBufferSize + 5,
        },
      }));
    });

    expect(onRender).toHaveBeenCalledTimes(2);
  });

  it('does not rerender table data selector on unrelated store updates', () => {
    const onRender = vi.fn();
    render(<TableDataProbe onRender={onRender} />);

    expect(onRender).toHaveBeenCalledTimes(1);

    act(() => {
      useScannersStore.setState((state) => ({
        stats: {
          ...state.stats,
          renderDurationP95Ms: state.stats.renderDurationP95Ms + 1,
        },
      }));
    });

    expect(onRender).toHaveBeenCalledTimes(1);
  });

  it('rerenders table data selector when selected fields change', () => {
    const onRender = vi.fn();
    render(<TableDataProbe onRender={onRender} />);

    expect(onRender).toHaveBeenCalledTimes(1);

    act(() => {
      useScannersStore.setState({ loading: true });
    });

    expect(onRender).toHaveBeenCalledTimes(2);
  });
});

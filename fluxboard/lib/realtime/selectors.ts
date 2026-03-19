export interface IndexRange {
  start: number;
  end: number;
}

export function clampIndexRange(count: number, range?: Partial<IndexRange> | null): IndexRange {
  if (count <= 0) {
    return { start: 0, end: 0 };
  }

  const rawStart = Math.max(0, Math.trunc(range?.start ?? 0));
  const rawEnd = Math.max(rawStart, Math.trunc(range?.end ?? count));
  return {
    start: Math.min(rawStart, count),
    end: Math.min(rawEnd, count),
  };
}

export function createVisibleRowsSelector<T>() {
  let previousRows: readonly T[] | null = null;
  let previousRange: IndexRange = { start: 0, end: 0 };
  let previousVersion = -1;
  let previousResult: readonly T[] = [];

  return (
    rows: readonly T[],
    range?: Partial<IndexRange> | null,
    version = 0,
  ): readonly T[] => {
    const nextRange = clampIndexRange(rows.length, range);
    if (
      previousRows === rows
      && previousRange.start === nextRange.start
      && previousRange.end === nextRange.end
      && previousVersion === version
    ) {
      return previousResult;
    }

    const nextResult = rows.slice(nextRange.start, nextRange.end);
    if (
      previousVersion === version
      && (
      previousResult.length === nextResult.length
      && nextResult.every((row, index) => row === previousResult[index])
      )
    ) {
      previousRows = rows;
      previousRange = nextRange;
      previousVersion = version;
      return previousResult;
    }

    previousRows = rows;
    previousRange = nextRange;
    previousVersion = version;
    previousResult = nextResult;
    return previousResult;
  };
}

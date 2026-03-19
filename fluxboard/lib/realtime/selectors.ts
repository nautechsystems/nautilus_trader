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
  let previousResult: readonly T[] = [];

  return (rows: readonly T[], range?: Partial<IndexRange> | null): readonly T[] => {
    const nextRange = clampIndexRange(rows.length, range);
    if (
      previousRows === rows
      && previousRange.start === nextRange.start
      && previousRange.end === nextRange.end
    ) {
      return previousResult;
    }

    const nextResult = rows.slice(nextRange.start, nextRange.end);
    if (
      previousResult.length === nextResult.length
      && nextResult.every((row, index) => row === previousResult[index])
    ) {
      previousRows = rows;
      previousRange = nextRange;
      return previousResult;
    }

    previousRows = rows;
    previousRange = nextRange;
    previousResult = nextResult;
    return previousResult;
  };
}

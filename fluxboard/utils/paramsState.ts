export function countDirtyCells(dirtyMap: Map<string, Set<string>>): number {
  let total = 0;
  for (const set of dirtyMap.values()) {
    total += set.size;
  }
  return total;
}

export function countDirtyInSelection(
  dirtyMap: Map<string, Set<string>>,
  selected: string[]
): number {
  let total = 0;
  for (const id of selected) {
    total += dirtyMap.get(id)?.size ?? 0;
  }
  return total;
}


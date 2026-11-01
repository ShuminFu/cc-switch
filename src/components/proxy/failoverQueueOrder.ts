import type { FailoverQueueItem } from "@/types/proxy";

/**
 * Sort-index updates that swap queue items `from` and `to`. The queue
 * follows the main provider list, so swapping the two sort indices moves
 * them past each other without disturbing providers outside the queue.
 * Returns `null` when the indices are missing or equal (nothing to swap).
 */
export function swapQueueSortIndex(
  queue: FailoverQueueItem[],
  from: number,
  to: number,
): { id: string; sortIndex: number }[] | null {
  const a = queue[from];
  const b = queue[to];
  if (!a || !b) return null;
  if (a.sortIndex == null || b.sortIndex == null) return null;
  if (a.sortIndex === b.sortIndex) return null;
  return [
    { id: a.providerId, sortIndex: b.sortIndex },
    { id: b.providerId, sortIndex: a.sortIndex },
  ];
}

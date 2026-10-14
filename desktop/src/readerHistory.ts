/** Bounded article-only back stack; snapshots are immutable, revision-scoped. */
export function appendReaderHistory<T>(stack: readonly T[], article: T): T[] {
  return [...stack.slice(-49), article];
}

export function popReaderHistory<T>(stack: readonly T[]): { previous: T | undefined; rest: T[] } {
  return { previous: stack[stack.length - 1], rest: stack.slice(0, -1) };
}

/** Bounded revision-scoped back stack; HTML snapshots consume memory. */
const MAX_HISTORY_ENTRIES = 12;
const MAX_HISTORY_CHARS = 4_000_000;

export function appendReaderHistory<T extends { html: string }>(stack: readonly T[], article: T): T[] {
  const entries = [...stack, article];
  const retained: T[] = [];
  let chars = 0;
  for (let index = entries.length - 1; index >= 0 && retained.length < MAX_HISTORY_ENTRIES; index--) {
    const entry = entries[index];
    if (chars + entry.html.length > MAX_HISTORY_CHARS) break;
    retained.push(entry);
    chars += entry.html.length;
  }
  return retained.reverse();
}

export function popReaderHistory<T>(stack: readonly T[]): { previous: T | undefined; rest: T[] } {
  return { previous: stack[stack.length - 1], rest: stack.slice(0, -1) };
}

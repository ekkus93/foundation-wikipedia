/** Bounded revision-scoped navigation snapshots; never refetch old revisions on Back/Forward. */
const MAX_HISTORY_ENTRIES = 12;
const MAX_HISTORY_CHARS = 4_000_000;

export interface ReaderNavigation<T> {
  current: T | null;
  back: T[];
  forward: T[];
}

export function emptyReaderNavigation<T>(): ReaderNavigation<T> {
  return { current: null, back: [], forward: [] };
}

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

/** Keep Back/Forward scoped to the actual immutable article edition, even
 * when a stale UI selector or asynchronous request tries to open another wiki.
 * Numeric page and revision IDs are not globally unique across editions. */
export function openReaderArticle<T extends { html: string; project: string }>(state: ReaderNavigation<T>, article: T): ReaderNavigation<T> {
  const sameEdition = !state.current || state.current.project === article.project;
  const previous = sameEdition ? state : emptyReaderNavigation<T>();
  return {
    current: article,
    back: previous.current ? appendReaderHistory(previous.back, previous.current) : previous.back,
    forward: [], // A new article creates a new branch in the navigation history.
  };
}

/** A cross-edition bookmark replaces navigation; same-edition bookmarks keep
 * the current revision on Back. Compare immutable article identities, never
 * the mutable edition selector, which may be changing asynchronously. */
export function openBookmarkedReaderArticle<T extends { html: string; project: string }>(
  state: ReaderNavigation<T>, article: T,
): ReaderNavigation<T> {
  // Both ordinary link navigation and bookmark reopening use the same
  // edition boundary, independent of the mutable UI selector.
  return openReaderArticle(state, article);
}

export function backReaderArticle<T extends { html: string }>(state: ReaderNavigation<T>): ReaderNavigation<T> {
  if (!state.current) return state;
  const { previous, rest } = popReaderHistory(state.back);
  return {
    current: previous ?? null, // Back from the first article returns to search results.
    back: rest,
    forward: appendReaderHistory(state.forward, state.current),
  };
}

export function forwardReaderArticle<T extends { html: string }>(state: ReaderNavigation<T>): ReaderNavigation<T> {
  const { previous, rest } = popReaderHistory(state.forward);
  if (!previous) return state;
  return {
    current: previous,
    back: state.current ? appendReaderHistory(state.back, state.current) : state.back,
    forward: rest,
  };
}

/** Keep only nearby frames mounted across Back/Forward to preserve browser-owned
 * in-iframe scroll where supported, without granting iframe scripts or origin access.
 * Older history stays navigable but its DOM/scroll is allowed to be evicted.
 */
export function mountedReaderFrames<T extends { frameId: number }>(state: ReaderNavigation<T>): T[] {
  const nearby = [
    ...state.back.slice(-2),
    ...(state.current ? [state.current] : []),
    ...state.forward.slice(-2),
  ];
  const seen = new Set<number>();
  return nearby.filter((item) => {
    if (seen.has(item.frameId)) return false;
    seen.add(item.frameId);
    return true;
  });
}

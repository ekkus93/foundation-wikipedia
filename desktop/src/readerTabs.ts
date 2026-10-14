/** Bounded in-memory desktop reader tabs. Each tab owns its mounted reader state. */
export const MAX_READER_TABS = 4;

export interface ReaderTab {
  id: number;
  title: string;
}

export interface ReaderTabsState {
  tabs: ReaderTab[];
  activeId: number;
  nextId: number;
}

export function initialReaderTabs(): ReaderTabsState {
  return { tabs: [{ id: 1, title: "New tab" }], activeId: 1, nextId: 2 };
}

export function addReaderTab(state: ReaderTabsState): ReaderTabsState {
  if (state.tabs.length >= MAX_READER_TABS) return state;
  const id = state.nextId;
  return { tabs: [...state.tabs, { id, title: "New tab" }], activeId: id, nextId: id + 1 };
}

export function selectReaderTab(state: ReaderTabsState, id: number): ReaderTabsState {
  if (state.activeId === id || !state.tabs.some((tab) => tab.id === id)) return state;
  return { ...state, activeId: id };
}

export function closeReaderTab(state: ReaderTabsState, id: number): ReaderTabsState {
  const index = state.tabs.findIndex((tab) => tab.id === id);
  // Always retain one available reader, and never close an unknown tab.
  if (index < 0 || state.tabs.length === 1) return state;
  const remaining = state.tabs.filter((tab) => tab.id !== id);
  const activeId = state.activeId === id
    ? remaining[Math.min(index, remaining.length - 1)].id
    : state.activeId;
  return { ...state, tabs: remaining, activeId };
}

/** Article labels are display-only; never interpret titles as URLs or HTML. */
export function titleReaderTab(state: ReaderTabsState, id: number, articleTitle: string | null): ReaderTabsState {
  const title = articleTitle?.trim().replace(/\s+/gu, " ").slice(0, 72) || "New tab";
  const tab = state.tabs.find((candidate) => candidate.id === id);
  if (!tab || tab.title === title) return state;
  return { ...state, tabs: state.tabs.map((candidate) => candidate.id === id ? { ...candidate, title } : candidate) };
}

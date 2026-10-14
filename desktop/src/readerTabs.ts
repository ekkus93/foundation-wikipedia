/** Bounded in-memory desktop reader tabs. Each tab owns its mounted reader state. */
export const MAX_READER_TABS = 4;

export interface ReaderTab {
  id: number;
}

export interface ReaderTabsState {
  tabs: ReaderTab[];
  activeId: number;
  nextId: number;
}

export function initialReaderTabs(): ReaderTabsState {
  return { tabs: [{ id: 1 }], activeId: 1, nextId: 2 };
}

export function addReaderTab(state: ReaderTabsState): ReaderTabsState {
  if (state.tabs.length >= MAX_READER_TABS) return state;
  const id = state.nextId;
  return { tabs: [...state.tabs, { id }], activeId: id, nextId: id + 1 };
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

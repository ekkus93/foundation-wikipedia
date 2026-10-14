import test from "node:test";
import assert from "node:assert/strict";
import {
  MAX_READER_TABS, addReaderTab, closeReaderTab, initialReaderTabs, selectReaderTab, titleReaderTab,
} from "../src/readerTabs.ts";

test("reader tabs are independent and IDs are never reused on close", () => {
  let state = initialReaderTabs();
  assert.deepEqual(state, { tabs: [{ id: 1, title: "New tab" }], activeId: 1, nextId: 2 });
  state = addReaderTab(state);
  state = addReaderTab(state);
  assert.deepEqual(state.tabs.map((tab) => tab.id), [1, 2, 3]);
  state = selectReaderTab(state, 1);
  state = closeReaderTab(state, 2);
  assert.equal(state.activeId, 1);
  state = addReaderTab(state);
  assert.deepEqual(state.tabs.map((tab) => tab.id), [1, 3, 4]);
  assert.equal(state.activeId, 4);
});

test("closing an active tab selects a surviving neighbor", () => {
  let state = addReaderTab(addReaderTab(initialReaderTabs()));
  state = closeReaderTab(state, 2);
  assert.equal(state.activeId, 3);
  state = closeReaderTab(state, 3);
  assert.equal(state.activeId, 1);
  assert.equal(closeReaderTab(state, 1), state);
});

test("invalid selections and unrecognized closures do not mutate state", () => {
  const state = initialReaderTabs();
  assert.equal(selectReaderTab(state, -1), state);
  assert.equal(selectReaderTab(state, 1), state);
  assert.equal(closeReaderTab(state, 99), state);
});

test("reader tab count is bounded without evicting existing reading sessions", () => {
  let state = initialReaderTabs();
  for (let i = 0; i < MAX_READER_TABS + 5; i++) state = addReaderTab(state);
  assert.equal(state.tabs.length, MAX_READER_TABS);
  assert.equal(state.activeId, MAX_READER_TABS);
  assert.equal(addReaderTab(state), state);
});

test("reader titles follow the current article without mutating other tabs", () => {
  const state = addReaderTab(initialReaderTabs());
  const renamed = titleReaderTab(state, 1, "  Article    with  spaces ");
  assert.equal(renamed.tabs[0].title, "Article with spaces");
  assert.equal(renamed.tabs[1].title, "New tab");
  assert.equal(titleReaderTab(renamed, 1, "Article with spaces"), renamed);
  assert.equal(titleReaderTab(renamed, 777, "Other"), renamed);
  assert.equal(titleReaderTab(renamed, 1, "x".repeat(200)).tabs[0].title.length, 72);
  assert.equal(titleReaderTab(renamed, 1, null).tabs[0].title, "New tab");
});

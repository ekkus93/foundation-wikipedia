import test from "node:test";
import assert from "node:assert/strict";
import {
  MAX_READER_TABS, adjacentReaderTabId, addReaderTab, canCloseReaderTab, closeReaderTab, initialReaderTabs, selectReaderTab, titleReaderTab,
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

test("long Unicode titles do not split surrogate pairs", () => {
  const state = initialReaderTabs();
  const title = "A".repeat(71) + "📚" + "B".repeat(20);
  const renamed = titleReaderTab(state, 1, title);
  assert.equal(renamed.tabs[0].title, "A".repeat(71) + "📚");
  assert.equal(Array.from(renamed.tabs[0].title).length, 72);
});

test("title truncation preserves combining marks and joined emoji graphemes", () => {
  const state = initialReaderTabs();
  assert.equal(titleReaderTab(state, 1, "A".repeat(71) + "e\u0301" + "Z").tabs[0].title, "A".repeat(71) + "e\u0301");
  const family = "👩‍👩‍👧‍👦";
  assert.equal(titleReaderTab(state, 1, "A".repeat(71) + family + "Z").tabs[0].title, "A".repeat(71) + family);
  assert.equal(titleReaderTab(state, 1, "e\u0301".repeat(80)).tabs[0].title, "e\u0301".repeat(72));
});


test("long source titles retain only the first 72 graphemes", () => {
  const state = initialReaderTabs();
  const prefix = "📚".repeat(71) + "e\u0301";
  const renamed = titleReaderTab(state, 1, prefix + "Z".repeat(100_000));
  assert.equal(renamed.tabs[0].title, prefix);
});

test("keyboard tab navigation wraps and skips closed IDs", () => {
  let state = addReaderTab(addReaderTab(initialReaderTabs()));
  state = closeReaderTab(state, 2);
  assert.equal(adjacentReaderTabId(state, 1, "ArrowRight"), 3);
  assert.equal(adjacentReaderTabId(state, 3, "ArrowRight"), 1);
  assert.equal(adjacentReaderTabId(state, 1, "ArrowLeft"), 3);
  assert.equal(adjacentReaderTabId(state, 3, "Home"), 1);
  assert.equal(adjacentReaderTabId(state, 1, "End"), 3);
  assert.equal(adjacentReaderTabId(state, 2, "Home"), null);
  assert.equal(adjacentReaderTabId(state, 1, "Escape"), null);
  assert.equal(adjacentReaderTabId(initialReaderTabs(), 1, "ArrowLeft"), 1);
});


test("only a real removable tab may release mounted reader resources", () => {
  const single = initialReaderTabs();
  assert.equal(canCloseReaderTab(single, 1), false);
  assert.equal(canCloseReaderTab(single, 99), false);
  const multiple = addReaderTab(single);
  assert.equal(canCloseReaderTab(multiple, 1), true);
  assert.equal(canCloseReaderTab(multiple, 2), true);
  assert.equal(canCloseReaderTab(multiple, 99), false);
  assert.equal(canCloseReaderTab(closeReaderTab(multiple, 2), 1), false);
});

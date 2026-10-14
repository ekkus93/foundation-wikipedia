import test from "node:test";
import assert from "node:assert/strict";
import {
  appendReaderHistory, backReaderArticle, emptyReaderNavigation,
  forwardReaderArticle, openReaderArticle, popReaderHistory,
} from "../src/readerHistory.ts";

const article = (pageId, revisionId = pageId * 10) => ({
  pageId, revisionId, title: String(pageId), html: "<p>" + pageId + "</p>",
});

test("Back and Forward preserve exact revisions without network refetch", () => {
  const first = article(1, 101);
  const second = article(2, 205);
  let state = openReaderArticle(emptyReaderNavigation(), first);
  state = openReaderArticle(state, second);
  state = backReaderArticle(state);
  assert.deepEqual(state.current, first);
  assert.deepEqual(state.forward, [second]);
  state = forwardReaderArticle(state);
  assert.deepEqual(state.current, second);
  assert.deepEqual(state.back, [first]);
});

test("Back from the first article returns to search with Forward available", () => {
  const first = article(1);
  let state = backReaderArticle(openReaderArticle(emptyReaderNavigation(), first));
  assert.equal(state.current, null);
  assert.deepEqual(state.forward, [first]);
  state = forwardReaderArticle(state);
  assert.deepEqual(state.current, first);
  assert.equal(state.forward.length, 0);
});

test("Opening a different article clears stale forward history", () => {
  let state = openReaderArticle(emptyReaderNavigation(), article(1));
  state = openReaderArticle(state, article(2));
  state = backReaderArticle(state);
  state = openReaderArticle(state, article(3));
  assert.equal(state.current.pageId, 3);
  assert.deepEqual(state.forward, []);
  assert.equal(forwardReaderArticle(state), state);
});

test("empty navigation and unavailable Back are harmless", () => {
  const state = emptyReaderNavigation();
  assert.equal(backReaderArticle(state), state);
  assert.equal(forwardReaderArticle(state), state);
  assert.deepEqual(popReaderHistory([]), { previous: undefined, rest: [] });
});

test("history retains only the 12 newest snapshots", () => {
  let stack = [];
  for (let id = 1; id <= 60; id++) stack = appendReaderHistory(stack, article(id));
  assert.equal(stack.length, 12);
  assert.equal(stack[0].pageId, 49);
  assert.equal(popReaderHistory(stack).previous.pageId, 60);
});

test("history drops old snapshots before exceeding the HTML memory budget", () => {
  const entry = (id, size) => ({ id, html: "x".repeat(size) });
  const stack = appendReaderHistory(appendReaderHistory([], entry(1, 3_000_000)), entry(2, 2_000_000));
  assert.deepEqual(stack.map((item) => item.id), [2]);
  assert.deepEqual(appendReaderHistory(stack, entry(3, 4_000_001)), []);
});

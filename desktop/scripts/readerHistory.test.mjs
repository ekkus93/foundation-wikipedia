import test from "node:test";
import assert from "node:assert/strict";
import { appendReaderHistory, popReaderHistory } from "../src/readerHistory.ts";

test("back navigation restores the exact previously viewed revision", () => {
  const first = { pageId: 1, revisionId: 100, title: "One" };
  const second = { pageId: 2, revisionId: 200, title: "Two" };
  const stack = appendReaderHistory(appendReaderHistory([], first), second);
  const popped = popReaderHistory(stack);
  assert.equal(popped.previous, second);
  assert.equal(popReaderHistory(popped.rest).previous, first);
  assert.deepEqual(popReaderHistory([]), { previous: undefined, rest: [] });
});

test("history is bounded to the 50 most recent articles", () => {
  let stack = [];
  for (let id = 1; id <= 60; id++) stack = appendReaderHistory(stack, id);
  assert.equal(stack.length, 50);
  assert.equal(stack[0], 11);
  assert.equal(popReaderHistory(stack).previous, 60);
});

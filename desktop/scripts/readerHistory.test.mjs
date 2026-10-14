import test from "node:test";
import assert from "node:assert/strict";
import { appendReaderHistory, popReaderHistory } from "../src/readerHistory.ts";

test("back navigation restores the exact previously viewed revision", () => {
  const first = { pageId: 1, revisionId: 100, title: "One", html: "first" };
  const second = { pageId: 2, revisionId: 200, title: "Two", html: "second" };
  const stack = appendReaderHistory(appendReaderHistory([], first), second);
  const popped = popReaderHistory(stack);
  assert.equal(popped.previous, second);
  assert.equal(popReaderHistory(popped.rest).previous, first);
  assert.deepEqual(popReaderHistory([]), { previous: undefined, rest: [] });
});

test("history retains only the 12 newest snapshots", () => {
  let stack = [];
  for (let id = 1; id <= 60; id++) stack = appendReaderHistory(stack, { id, html: "x" });
  assert.equal(stack.length, 12);
  assert.equal(stack[0].id, 49);
  assert.equal(popReaderHistory(stack).previous.id, 60);
});

test("history drops old snapshots before exceeding the HTML memory budget", () => {
  const entry = (id, size) => ({ id, html: "x".repeat(size) });
  const stack = appendReaderHistory(appendReaderHistory([], entry(1, 3_000_000)), entry(2, 2_000_000));
  assert.deepEqual(stack.map((item) => item.id), [2]);
  assert.deepEqual(appendReaderHistory(stack, entry(3, 4_000_001)), []);
});

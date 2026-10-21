import test from "node:test";
import assert from "node:assert/strict";
import { parseReaderArticle, parseReaderSearchHits } from "../src/readerResponse.ts";

test("accepts bounded revision-scoped article and search IPC responses", () => {
  assert.deepEqual(parseReaderSearchHits([[12, "Physics"], [13, "Mathematics"]]), [[12, "Physics"], [13, "Mathematics"]]);
  assert.deepEqual(parseReaderArticle([12, 345, "Physics", "<p>safe</p>"], 12), [12, 345, "Physics", "<p>safe</p>"]);
});

test("rejects malformed, duplicate and unsafe search identities", () => {
  for (const value of [null, {}, [[0, "Physics"]], [[1.5, "Physics"]], [[Number.MAX_SAFE_INTEGER + 1, "Physics"]],
    [[1, ""]], [[1, " Bad"]], [[1, "A\nB"]], [[1, "A\u202eB"]],
    [[1, "Physics"], [1, "Math"]], [[1, "Physics", "extra"]], Array(101).fill([1, "A"])]) {
    assert.throws(() => parseReaderSearchHits(value), /Invalid Wikipedia/);
  }
});

test("rejects invalid revisions, mismatched page IDs and oversized previews", () => {
  for (const value of [null, [1, 2, "A"], [0, 2, "A", ""], [1, 0, "A", ""],
    [1, 2, "A\u202eB", ""], [1, 2, "A", 4], [1, 2, "A", "é".repeat(1_000_001)]]) {
    assert.throws(() => parseReaderArticle(value), /Invalid Wikipedia/);
  }
  assert.throws(() => parseReaderArticle([2, 3, "B", ""], 1), /Invalid Wikipedia/);
});

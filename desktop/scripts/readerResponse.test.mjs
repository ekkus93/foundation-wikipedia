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

test("rejects sparse search result arrays rather than skipping missing identities", () => {
  const sparse = new Array(2);
  sparse[1] = [1, "Physics"];
  assert.throws(() => parseReaderSearchHits(sparse), /Invalid Wikipedia search identity/);
});

test("rejects invalid revisions, mismatched page IDs and oversized previews", () => {
  for (const value of [null, [1, 2, "A"], [0, 2, "A", ""], [1, 0, "A", ""],
    [1, 2, "A\u202eB", ""], [1, 2, "A", 4], [1, 2, "A", "é".repeat(1_000_001)]]) {
    assert.throws(() => parseReaderArticle(value), /Invalid Wikipedia/);
  }
  assert.throws(() => parseReaderArticle([2, 3, "B", ""], 1), /Invalid Wikipedia/);
});

test("rejects cross-page canonical title collisions after first-letter and underscore normalization", () => {
  for (const hits of [
    [[1, "Physics"], [2, "physics"]],
    [[1, "Quantum_mechanics"], [2, "Quantum mechanics"]],
    [[1, "äpfel"], [2, "Äpfel"]],
    [[1, "Physics"], [2, "physics"], [3, "Chemistry"]],
  ]) {
    assert.throws(() => parseReaderSearchHits(hits), /Conflicting Wikipedia search titles/);
  }
  // Wikipedia's title key only uppercases the first scalar, not the rest.
  assert.deepEqual(parseReaderSearchHits([[1, "iPhone"], [2, "Iphone"]]), [[1, "iPhone"], [2, "Iphone"]]);
  // Multi-codepoint first-scalar uppercase expansions are left as-is.
  assert.deepEqual(parseReaderSearchHits([[1, "ßeta"], [2, "SSeta"]]), [[1, "ßeta"], [2, "SSeta"]]);
});

test("rejects invisible directional marks in IPC titles without rejecting RTL text", () => {
  for (const mark of ["\u061c", "\u200e", "\u200f"]) {
    assert.throws(() => parseReaderSearchHits([[1, "Phy" + mark + "sics"]]), /Invalid Wikipedia search identity/);
    assert.throws(() => parseReaderArticle([1, 2, "Phy" + mark + "sics", "<p>text</p>"]), /Invalid Wikipedia article identity/);
  }
  assert.deepEqual(parseReaderSearchHits([[1, "פיזיקה"]]), [[1, "פיזיקה"]]);
  assert.deepEqual(parseReaderArticle([1, 2, "פיזיקה", "<p>text</p>"]), [1, 2, "פיזיקה", "<p>text</p>"]);
});

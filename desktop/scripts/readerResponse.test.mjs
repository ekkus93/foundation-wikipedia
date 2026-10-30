import test from "node:test";
import assert from "node:assert/strict";
import { parseReaderArticle, parseReaderSearchHits, parseReaderBookmarks, parseReaderVisits, parseReaderSavedPanel, validReaderTitleInput, normalizedReaderSearchQuery } from "../src/readerResponse.ts";

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

test("exact-title user navigation validates directional marks and UTF-8 byte limits", () => {
  for (const title of ["Physics", "الفيزياء", "פיזיקה", "Mathematics"]) {
    assert.equal(validReaderTitleInput(title), true, title);
  }
  for (const title of ["", "  ", " Physics", "Physics ", "A\u202eB",
    "Physics\u2066", "A\nB", "é".repeat(257), "x".repeat(513)]) {
    assert.equal(validReaderTitleInput(title), false, JSON.stringify(title));
  }
  assert.equal(validReaderTitleInput("é".repeat(256)), true);
  assert.equal(validReaderTitleInput(null), false);
});

test("rejects Rust-disallowed invisible and noncharacter title identities", () => {
  for (const marker of ["\u200b", "\u2028", "\u2029", "\u2060", "\u206a", "\u206f",
    "\ufeff", "\ufdd0", "\ufdef", "\ufffe", "\uffff", "\u{1fffe}", "\u{10ffff}"]) {
    const title = "Physics" + marker;
    assert.equal(validReaderTitleInput(title), false, JSON.stringify(title));
    assert.throws(() => parseReaderSearchHits([[42, title]]), /Invalid Wikipedia search identity/);
    assert.throws(() => parseReaderArticle([42, 333, title, "<p>text</p>"]), /Invalid Wikipedia article identity/);
  }
  for (const title of ["می\u200cروم", "क्\u200dष", "🐈", "פיזיקה"]) {
    assert.equal(validReaderTitleInput(title), true, title);
  }
});

test("online search trims ordinary outer spacing but validates the exact UTF-8 query boundary", () => {
  assert.equal(normalizedReaderSearchQuery("  Quantum mechanics  "), "Quantum mechanics");
  assert.equal(normalizedReaderSearchQuery("  猫と🐈  "), "猫と🐈");
  assert.equal(normalizedReaderSearchQuery("é".repeat(256)), "é".repeat(256));
  for (const invalid of [
    null, undefined, 42, "", "   ", "é".repeat(257), "猫".repeat(171),
    "Physics\nMath", "Physics\u200b", "Physics\u202e",
    "Physics\u2060", "Physics\u{1fffe}", "Physics\ud800",
  ]) {
    assert.equal(normalizedReaderSearchQuery(invalid), null, JSON.stringify(invalid));
  }
});

test("bounded bookmark listing validates exact identities without treating them as article bytes", () => {
  assert.deepEqual(parseReaderBookmarks([["enwiki", 42, 333], ["frwiki", 42, 991]]),
    [["enwiki", 42, 333], ["frwiki", 42, 991]]);
  const sparse = Array(2);
  sparse[1] = ["enwiki", 42, 333];
  for (const bad of [null, {}, sparse, [["dewiki", 0, 2]],
    [["enwiki", 42, 0]], [["untrustedwiki", 42, 3]],
    [["enwiki", 42, 3], ["enwiki", 42, 4]], Array(101).fill(["enwiki", 42, 3])]) {
    assert.throws(() => parseReaderBookmarks(bad), /Invalid bookmark|Duplicate bookmark/);
  }
});

test("rejects malformed UTF-16 title identities before IPC or HTTP encoding", () => {
  for (const invalidTitle of ["\\ud800", "\\udc00", "Phy\\ud800sics", "\\ud800\\ud800", "\\ud800A"]) {
    const title = invalidTitle.replaceAll("\\ud800", "\uD800").replaceAll("\\udc00", "\uDC00");
    assert.equal(validReaderTitleInput(title), false);
    assert.throws(() => parseReaderSearchHits([[42, title]]), /Invalid Wikipedia search identity/);
    assert.throws(() => parseReaderArticle([42, 80, title, "<p>preview</p>"]), /Invalid Wikipedia article identity/);
  }
  // Properly paired surrogate code units represent valid Unicode scalar values.
  assert.equal(validReaderTitleInput("🐈"), true);
  assert.deepEqual(parseReaderSearchHits([[42, "猫と🐈"]]), [[42, "猫と🐈"]]);
});

test("opt-in reading history IPC accepts repeated visits but rejects malformed records", () => {
  const entries = [["enwiki", 42, 123, 1700000000], ["enwiki", 42, 123, 1700000001]];
  assert.deepEqual(parseReaderVisits(entries), entries);
  const sparse = new Array(2);
  sparse[1] = entries[0];
  for (const bad of [null, {}, sparse, [["enwiki", 42, 1]],
    [["wrongwiki", 42, 1, 1]], [["enwiki", 0, 1, 1]], [["enwiki", 42, 0, 1]],
    [["enwiki", 42, 1, 0]], [["enwiki", 42, 1, Number.NaN]],
    [["enwiki", 42, 1, 253402300800]], Array(51).fill(entries[0])]) {
    assert.throws(() => parseReaderVisits(bad), /Invalid reading history/);
  }
});


test("saved panel response rejects mixed or incomplete IPC tuples as a unit", () => {
  const bookmarks = [["enwiki", 42, 333]];
  const visits = [["enwiki", 42, 333, 1700000000]];
  assert.deepEqual(parseReaderSavedPanel([bookmarks, true, visits]), [bookmarks, true, visits]);
  assert.deepEqual(parseReaderSavedPanel([[], false, []]), [[], false, []]);
  for (const invalid of [null, {}, [bookmarks, false], [bookmarks, false, visits, []],
    [bookmarks, "false", visits], [bookmarks, false, [["enwiki", 0, 333, 1700000000]]],
    [[["enwiki", 42, 0]], false, visits], [bookmarks, false, null]]) {
    assert.throws(() => parseReaderSavedPanel(invalid), /Invalid/);
  }
});

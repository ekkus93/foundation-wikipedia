import test from "node:test";
import assert from "node:assert/strict";
import { bookmarkSavedText } from "../src/bookmarkSavedText.ts";

test("a saved bookmark shows its date from the formatter", () => {
  let seen;
  const text = bookmarkSavedText(1795651200, (date) => { seen = date.getTime(); return "26 Nov 2026"; });
  assert.equal(text, "Saved 26 Nov 2026");
  assert.equal(seen, 1795651200000);
});

test("a bookmark saved before dates were kept shows nothing", () => {
  for (const missing of [null, 0, -1, Number.NaN, 1.5]) {
    assert.equal(bookmarkSavedText(missing, () => { throw new Error("must not format"); }), "");
  }
});

test("the default formatter uses the platform locale and includes the year", () => {
  assert.match(bookmarkSavedText(1795651200), /^Saved .*2026/);
});

import test from "node:test";
import assert from "node:assert/strict";
import {
  isReaderTextSize, isReaderWidth, parseReaderLayout, isolatedReaderDocument,
} from "../src/readerLayout.ts";

test("reading layout accepts only bounded enumerated persisted values", () => {
  assert.deepEqual(parseReaderLayout(["standard", "narrow"]), ["standard", "narrow"]);
  assert.deepEqual(parseReaderLayout(["large", "wide"]), ["large", "wide"]);
  for (const bad of [null, {}, [], ["small"], ["small", "wide", "extra"],
    ["huge", "narrow"], ["large", "100vw"], ["small", null],
    ["small", "wide; background:url(https://example.com)"]]) {
    assert.throws(() => parseReaderLayout(bad), /Invalid saved reading layout/);
  }
  assert.equal(isReaderTextSize("small"), true);
  assert.equal(isReaderTextSize("21px"), false);
  assert.equal(isReaderWidth("wide"), true);
  assert.equal(isReaderWidth(undefined), false);
});

test("isolated preview changes size and width but never interpolates untrusted CSS", () => {
  const article = "<h1>Physics</h1><p>Body</p>";
  const narrow = isolatedReaderDocument(article, "small", "narrow");
  const wide = isolatedReaderDocument(article, "large", "wide");
  const standard = isolatedReaderDocument(article, "standard", "standard");
  assert.match(narrow, /font:15px\/1\.6/);
  assert.match(narrow, /max-width:58ch/);
  assert.match(wide, /font:20px\/1\.6/);
  assert.match(wide, /max-width:96ch/);
  assert.match(standard, /font:17px\/1\.6/);
  assert.match(standard, /max-width:74ch/);
  assert.match(wide, /default-src 'none'/);
  assert.match(wide, /frame-src 'none'/);
  assert.match(wide, /<h1>Physics<\/h1>/);
  const attack = "large; background:url(https://example.com)";
  const safe = isolatedReaderDocument(article, attack, attack);
  assert.equal(safe, standard);
  assert.equal(safe.includes(attack), false);
  assert.equal(safe.includes("https://example.com"), false);
});

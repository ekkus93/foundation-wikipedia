import test from "node:test";
import assert from "node:assert/strict";
import { normalizeWikiHref } from "../src/articleLinks.ts";

test("accepts only same-edition article links and normalizes titles", () => {
  assert.equal(normalizeWikiHref("./Albert_Einstein", "enwiki"), "Albert Einstein");
  assert.equal(normalizeWikiHref("/wiki/Gravit%C3%A4t#History", "dewiki"), "Gravität");
  assert.equal(normalizeWikiHref("https://fr.wikipedia.org/wiki/Physique", "frwiki"), "Physique");
  assert.equal(normalizeWikiHref("//ja.wikipedia.org/wiki/%E6%95%B0%E5%AD%A6", "jawiki"), "数学");
});

test("rejects external, unsafe, cross-edition and non-article destinations", () => {
  for (const href of [
    "javascript:alert(1)", "data:text/html,evil", "https://en.wikipedia.org.evil.test/wiki/Physics",
    "https://de.wikipedia.org/wiki/Physics", "http://en.wikipedia.org/wiki/Physics",
    "https://evil@en.wikipedia.org/wiki/Physics", "/w/index.php?title=Physics",
    "./File:Example.svg", "./Category:Physics", "./Talk:Physics", "#Section",
    "./Physics?redirect=no", "./%ZZ", "./%2e%2e/Secret", "./Physics\\evil",
    " ./Physics", "./Physics\n", "/wiki/", "./Physics:Special",
  ]) assert.equal(normalizeWikiHref(href, "enwiki"), null, href);
  assert.equal(normalizeWikiHref("./Physics", "xxwiki"), null);
});

test("accepts fragments as article-only navigation, not privileged iframe navigation", () => {
  assert.equal(normalizeWikiHref("./Quantum_mechanics#History", "enwiki"), "Quantum mechanics");
});

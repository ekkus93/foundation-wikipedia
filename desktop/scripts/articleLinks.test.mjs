import test from "node:test";
import assert from "node:assert/strict";
import { extractWikiArticleLinks, normalizeWikiHref } from "../src/articleLinks.ts";

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
    "/wiki/Physics/../Chemistry", "/wiki/Physics/%2e%2e/Chemistry",
    "https://en.wikipedia.org/wiki/%2e/Physics",
    "//en.wikipedia.org/wiki/Physics/.%2e/Chemistry",
    "./Physics/%2F/Chemistry", "./Physics/%5c/Chemistry",
    "./Physics/%252f/Chemistry", "./Physics/%255c/Chemistry",
    "./Physics/%252e%252e/Chemistry",
    "./Physics/%25252f/Chemistry", "./Physics/%2525255c/Chemistry",
    "./Physics/%25252e%25252e/Chemistry",
    "https://en.wikipedia.org/wiki/Physics/%252e%252e/Chemistry",
    " ./Physics", "./Physics\n", "/wiki/", "./Physics:Special",
  ]) assert.equal(normalizeWikiHref(href, "enwiki"), null, href);
  assert.equal(normalizeWikiHref("./Physics", "xxwiki"), null);
});

test("accepts fragments as article-only navigation, not privileged iframe navigation", () => {
  assert.equal(normalizeWikiHref("./Quantum_mechanics#History", "enwiki"), "Quantum mechanics");
});


test("extracts article links from an uninstantiated template rather than DOMParser", () => {
  const originalDocument = globalThis.document;
  let templateCreated = 0;
  let parsed = "";
  const anchor = (href, label) => ({
    getAttribute: (name) => name === "href" ? href : null,
    textContent: label,
  });
  const nodes = [
    anchor("./Physics", " Physics "),
    anchor("javascript:alert(1)", "unsafe"),
    anchor("./Physics", "duplicate"),
    anchor("./physics", "case alias"),
    anchor("./Gravit%C3%A4t", " Gravität "),
    anchor("./Chemistry", "Che\u202emistry"),
    anchor("./Biology", "Bio\u0085logy"),
    anchor("./Mathematics", "رياضيات"),
  ];
  globalThis.document = {
    createElement(tag) {
      assert.equal(tag, "template");
      templateCreated += 1;
      return {
        set innerHTML(html) { parsed = html; },
        content: {
          querySelectorAll(selector) {
            assert.equal(selector, "a[href]");
            return nodes;
          },
        },
      };
    },
  };
  try {
    const html = '<img src="https://example.invalid/track"><a href="./Physics">Physics</a>';
    assert.deepEqual(extractWikiArticleLinks(html, "enwiki"), [
      { title: "Physics", label: "Physics" },
      { title: "Gravität", label: "Gravität" },
      { title: "Chemistry", label: "Chemistry" },
      { title: "Biology", label: "Bio logy" },
      { title: "Mathematics", label: "رياضيات" },
    ]);
    assert.equal(parsed, html);
    assert.equal(templateCreated, 1);
    assert.deepEqual(extractWikiArticleLinks(html, "unknown"), []);
    assert.deepEqual(extractWikiArticleLinks("x".repeat(2_000_001), "enwiki"), []);
    // UTF-16 length is below the quota, but UTF-8 payload size exceeds 2 MB.
    assert.deepEqual(extractWikiArticleLinks("é".repeat(1_000_001), "enwiki"), []);
    assert.deepEqual(extractWikiArticleLinks("😀".repeat(600_000), "enwiki"), []);
    assert.equal(templateCreated, 1, "invalid inputs must not parse HTML");
  } finally {
    if (originalDocument === undefined) delete globalThis.document;
    else globalThis.document = originalDocument;
  }
});

test("rejects directional-format and C1 control characters in article-link destinations", () => {
  for (const href of [
    "./Phy\\u202esics",
    "./Physics\\u2066",
    "./Physics\\u2069",
    "./Physics\\u0085",
    "./Physics\u061c", "./Physics\u200e", "./Physics\u200f",
    "./Physics%D8%9C", "./Physics%E2%80%8E", "./Physics%E2%80%8F",
    "./Physics%E2%80%AE",
    "./Physics%E2%81%A6",
    "/wiki/Physics%E2%81%A9",
    "https://en.wikipedia.org/wiki/Physics%E2%80%AC",
  ]) {
    assert.equal(normalizeWikiHref(href, "enwiki"), null, href);
  }
  // Natural right-to-left letters without directional format controls are legitimate.
  assert.equal(normalizeWikiHref("./%D7%A4%D7%99%D7%96%D7%99%D7%A7%D7%94", "enwiki"), "פיזיקה");
});

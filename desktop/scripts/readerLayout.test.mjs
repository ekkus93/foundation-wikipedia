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


test("sandboxed preview honors explicit and system themes without CSS injection", () => {
  const html = "<p>Physics</p>";
  const light = isolatedReaderDocument(html, "standard", "standard", "light");
  const dark = isolatedReaderDocument(html, "standard", "standard", "dark");
  const system = isolatedReaderDocument(html, "standard", "standard", "system");
  assert.match(light, /html\{background:#ffffff\}/);
  assert.match(light, /color:#202122/);
  assert.match(dark, /html\{background:#202122\}/);
  assert.match(dark, /color:#eaecf0/);
  assert.match(system, /prefers-color-scheme: dark/);
  assert.match(system, /color:#eaecf0/);
  assert.doesNotMatch(light, /prefers-color-scheme/);
  assert.doesNotMatch(dark, /prefers-color-scheme/);
  const injection = "dark; background:url(https://example.com)";
  const safe = isolatedReaderDocument(html, "standard", "standard", injection);
  assert.equal(safe, light);
  assert.doesNotMatch(safe, /example.com/);
  assert.match(safe, /default-src 'none'/);
});

test("online article documents load only Wikimedia images and the hash-pinned link script", async () => {
  const { LINK_MESSAGE_KEY, LINK_SCRIPT, LINK_SCRIPT_HASH, IMAGE_SOURCES } = await import("../src/readerLayout.ts");
  const { createHash } = await import("node:crypto");
  const { readFileSync } = await import("node:fs");
  const digest = createHash("sha256").update(LINK_SCRIPT).digest("base64");
  assert.equal(LINK_SCRIPT_HASH, `'sha256-${digest}'`);
  // The srcdoc frame inherits the app window's CSP, which must allow the
  // same script hash and image hosts (and nothing broader).
  const csp = JSON.parse(readFileSync(new URL("../src-tauri/tauri.conf.json", import.meta.url))).app.security.csp;
  assert.match(csp, new RegExp(`script-src 'self' ${LINK_SCRIPT_HASH.replace(/[+/=]/g, "\\$&")};`));
  assert.ok(csp.includes(`img-src 'self' data: ${IMAGE_SOURCES};`), csp);
  assert.doesNotMatch(csp, /unsafe-inline'[^;]*script|script-src[^;]*unsafe/);

  const html = "<p>Physics</p>";
  const online = isolatedReaderDocument(html, "standard", "standard", "light", { images: true, links: true });
  assert.match(online, /img-src https:\/\/upload\.wikimedia\.org https:\/\/thumb\.wikimedia\.org https:\/\/wikimedia\.org;/);
  assert.ok(online.includes(`script-src ${LINK_SCRIPT_HASH}`));
  assert.ok(online.includes(`<script>${LINK_SCRIPT}</script>`));
  assert.match(online, new RegExp(LINK_MESSAGE_KEY));
  assert.doesNotMatch(online, /pointer-events:none/);
  assert.match(online, /\.infobox\{float:right/);
  // Offline or text-only: no images, no script, inert links.
  const offline = isolatedReaderDocument(html, "standard", "standard", "light", { images: false });
  assert.match(offline, /img-src 'none'/);
  assert.match(offline, /script-src 'none'/);
  assert.doesNotMatch(offline, /<script/);
  assert.match(offline, /a\{pointer-events:none\}/);
  for (const bad of [1, "yes", null]) {
    const doc = isolatedReaderDocument(html, "standard", "standard", "light", { links: bad });
    assert.match(doc, /script-src 'none'/);
  }
});

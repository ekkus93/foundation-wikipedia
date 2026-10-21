import test from "node:test";
import assert from "node:assert/strict";
import { revisionSourceUrl } from "../src/articleProvenance.ts";

test("exact-revision provenance uses only the correct Wikimedia host", () => {
  for (const [project, host] of [
    ["enwiki", "en.wikipedia.org"], ["dewiki", "de.wikipedia.org"],
    ["frwiki", "fr.wikipedia.org"], ["eswiki", "es.wikipedia.org"],
    ["jawiki", "ja.wikipedia.org"],
  ]) {
    const url = revisionSourceUrl(project, 42, 333);
    assert.equal(url, `https://${host}/w/index.php?oldid=333`);
    assert.equal(new URL(url).searchParams.size, 1);
  }
});

test("rejects forged hosts, prototypes and invalid article identities", () => {
  for (const project of ["", "ENWIKI", "commonswiki", "enwiki.evil.test", "__proto__", "constructor", null, 2]) {
    assert.equal(revisionSourceUrl(project, 42, 333), null);
  }
  for (const bad of [0, -1, 1.2, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1, "42", null]) {
    assert.equal(revisionSourceUrl("enwiki", bad, 333), null);
    assert.equal(revisionSourceUrl("enwiki", 42, bad), null);
  }
});

import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { auditInstalledPackages, licenseError } from "./audit-licenses.mjs";

test("license policy rejects missing and unknown declarations", () => {
  assert.equal(licenseError({ name: "ok", version: "1", license: "MIT" }), null);
  assert.match(licenseError({ name: "unknown", version: "1", license: "GPL-3.0-only" }), /unapproved/);
  assert.match(licenseError({ name: "missing", version: "1" }), /missing license/);
  assert.match(licenseError({ name: "", version: null }), /Malformed/);
});

test("audits scoped and nested npm packages rather than only direct dependencies", async () => {
  const root = await mkdtemp(join(tmpdir(), "wiki-npm-licenses-"));
  try {
    const nodeModules = join(root, "node_modules");
    const scoped = join(nodeModules, "@example", "licensed");
    const nested = join(scoped, "node_modules", "bad");
    await mkdir(nested, { recursive: true });
    await writeFile(join(scoped, "package.json"), JSON.stringify({
      name: "@example/licensed", version: "1", license: "Apache-2.0",
    }));
    await writeFile(join(nested, "package.json"), JSON.stringify({
      name: "bad", version: "1", license: "GPL-3.0-only",
    }));
    const result = await auditInstalledPackages(nodeModules);
    assert.equal(result.scanned, 2);
    assert.equal(result.errors.length, 1);
    assert.match(result.errors[0], /bad@1/);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

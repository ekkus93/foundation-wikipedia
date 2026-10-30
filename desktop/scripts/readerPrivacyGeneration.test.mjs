import test from "node:test";
import assert from "node:assert/strict";
import { ReaderPrivacyGenerationMonitor } from "../src/readerPrivacyGeneration.ts";

test("focus-time privacy monitor distinguishes initial, unchanged and reset generations", () => {
  const monitor = new ReaderPrivacyGenerationMonitor();
  assert.equal(monitor.observe("0"), "initial");
  assert.equal(monitor.observe("0"), "unchanged");
  assert.equal(monitor.observe("1"), "changed");
  assert.equal(monitor.observe("1"), "unchanged");
  assert.equal(monitor.observe("9223372036854775807"), "changed");
});

test("invalid or rounded SQLite generation never replaces the last accepted value", () => {
  const monitor = new ReaderPrivacyGenerationMonitor();
  assert.equal(monitor.observe("7"), "initial");
  for (const value of [7, null, "", "07", "-1", "1.0", "1e2", " 8", "9223372036854775808"]) {
    assert.equal(monitor.observe(value), "invalid");
  }
  assert.equal(monitor.observe("7"), "unchanged");
  assert.equal(monitor.observe("8"), "changed");
});

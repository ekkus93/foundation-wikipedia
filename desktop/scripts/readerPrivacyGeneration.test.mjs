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

test("out-of-order older generations cannot roll back a privacy reset", () => {
  const monitor = new ReaderPrivacyGenerationMonitor();
  assert.equal(monitor.observe("8"), "initial");
  assert.equal(monitor.observe("10"), "changed");
  assert.equal(monitor.observe("9"), "stale");
  assert.equal(monitor.observe("10"), "unchanged");
  assert.equal(monitor.observe("11"), "changed");
  assert.equal(monitor.observe("0"), "stale");
  assert.equal(monitor.observe("9223372036854775807"), "changed");
  assert.equal(monitor.observe("9223372036854775806"), "stale");
});

test("generation changes remain pending while a local reset owns the session", () => {
  const monitor = new ReaderPrivacyGenerationMonitor();
  assert.equal(monitor.observe("4"), "initial");
  assert.equal(monitor.observe("5", false), "deferred");
  assert.equal(monitor.observe("4"), "unchanged");
  assert.equal(monitor.observe("5", true), "changed");
  assert.equal(monitor.observe("5"), "unchanged");

  const fresh = new ReaderPrivacyGenerationMonitor();
  assert.equal(fresh.observe("7", false), "deferred");
  assert.equal(fresh.observe("7", true), "initial");
  assert.equal(fresh.observe("8", false), "deferred");
  assert.equal(fresh.observe("8"), "changed");
});

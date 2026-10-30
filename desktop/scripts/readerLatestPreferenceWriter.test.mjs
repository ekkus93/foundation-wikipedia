import test from "node:test";
import assert from "node:assert/strict";
import { ReaderLatestPreferenceWriter } from "../src/readerLatestPreferenceWriter.ts";

test("rapid theme changes serialize persistence in the order selected", async () => {
  const queue = new ReaderLatestPreferenceWriter();
  const started = [];
  let persisted = "system";
  let releaseFirst;
  const delayed = new Promise((resolve) => { releaseFirst = resolve; });
  const failures = [];
  const write = async (theme) => {
    started.push(theme);
    if (theme === "dark") await delayed;
    persisted = theme;
  };
  const first = queue.submit("dark", write, () => failures.push("dark"));
  const second = queue.submit("light", write, () => failures.push("light"));
  await Promise.resolve();
  assert.deepEqual(started, ["dark"], "the second native write must not overlap the first");
  releaseFirst();
  await Promise.all([first, second]);
  assert.deepEqual(started, ["dark", "light"]);
  assert.equal(persisted, "light", "the last user selection must win on disk");
  assert.deepEqual(failures, []);
});

test("superseded write failure cannot display an error or block the newer choice", async () => {
  const queue = new ReaderLatestPreferenceWriter();
  const saved = [];
  const errors = [];
  let rejectOld;
  const pending = new Promise((_, reject) => { rejectOld = reject; });
  const old = queue.submit("dark", () => pending, () => errors.push("stale error"));
  const latest = queue.submit("light", async (value) => { saved.push(value); }, () => errors.push("latest error"));
  await Promise.resolve();
  rejectOld(new Error("failed old write"));
  await Promise.all([old, latest]);
  assert.deepEqual(errors, []);
  assert.deepEqual(saved, ["light"]);
});

test("the latest failed write is reported and a later choice can recover", async () => {
  const queue = new ReaderLatestPreferenceWriter();
  const errors = [];
  await queue.submit("dark", async () => { throw new Error("storage unavailable"); }, () => errors.push("dark"));
  assert.deepEqual(errors, ["dark"]);
  await queue.submit("system", async () => {}, () => errors.push("system"));
  assert.deepEqual(errors, ["dark"], "recovery must not add a false error");
});

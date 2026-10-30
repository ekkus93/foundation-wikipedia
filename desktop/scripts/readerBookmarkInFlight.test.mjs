import test from "node:test";
import assert from "node:assert/strict";
import { ReaderBookmarkInFlight } from "../src/readerBookmarkInFlight.ts";

test("privacy reset releases an unresolved bookmark without unlocking a newer request", () => {
  const gate = new ReaderBookmarkInFlight();
  const old = gate.begin();
  assert.notEqual(old, null);
  assert.equal(gate.begin(), null);
  gate.invalidate();
  const fresh = gate.begin();
  assert.notEqual(fresh, null);
  assert.notEqual(fresh, old);
  gate.finish(old);
  assert.equal(gate.begin(), null, "old finally must not unlock a newer IPC");
  gate.finish(fresh);
  const next = gate.begin();
  assert.notEqual(next, null);
  gate.finish(next);
});

test("a stale failure after repeated privacy resets cannot change the active gate", async () => {
  const gate = new ReaderBookmarkInFlight();
  const old = gate.begin();
  let rejectOld;
  const pending = new Promise((_, reject) => { rejectOld = reject; });
  const completion = pending.catch(() => {}).finally(() => gate.finish(old));
  gate.invalidate();
  const discarded = gate.begin();
  gate.invalidate();
  const current = gate.begin();
  gate.finish(discarded);
  rejectOld(new Error("stale IPC failure"));
  await completion;
  assert.equal(gate.begin(), null);
  gate.finish(current);
  assert.notEqual(gate.begin(), null);
});

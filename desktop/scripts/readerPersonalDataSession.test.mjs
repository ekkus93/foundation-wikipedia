import test from "node:test";
import assert from "node:assert/strict";
import { ReaderPersonalDataSession } from "../src/readerPersonalDataSession.ts";

test("privacy reset rejects old bookmark completions and new writes during deletion", () => {
  const session = new ReaderPersonalDataSession();
  const before = session.snapshot();
  assert.equal(session.canWrite(), true);
  assert.equal(session.accepts(before), true);
  assert.equal(session.beginReset(), true);
  assert.equal(session.beginReset(), false);
  assert.equal(session.canWrite(), false);
  assert.equal(session.accepts(before), false);
  assert.equal(session.accepts(session.snapshot()), false);
  session.finishReset();
  assert.equal(session.canWrite(), true);
  assert.equal(session.accepts(before), false);
  assert.equal(session.accepts(session.snapshot()), true);
});

test("failed reset still invalidates stale completions but permits later writes", () => {
  const session = new ReaderPersonalDataSession();
  const stale = session.snapshot();
  session.beginReset();
  session.finishReset();
  assert.equal(session.accepts(stale), false);
  const fresh = session.snapshot();
  session.beginReset();
  session.finishReset();
  assert.equal(session.accepts(fresh), false);
  assert.equal(session.accepts(session.snapshot()), true);
});

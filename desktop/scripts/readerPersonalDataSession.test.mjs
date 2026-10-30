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

test("delayed personal-data action completions cannot change a post-reset UI session", async () => {
  const session = new ReaderPersonalDataSession();
  const revision = session.snapshot();
  let finish;
  const pending = new Promise((resolve) => { finish = resolve; });
  const displayed = { bookmarks: ["new"], visits: ["new"], historyEnabled: false, busy: true };
  const completion = pending.then(() => {
    if (session.accepts(revision)) {
      displayed.bookmarks = [];
      displayed.visits = [];
      displayed.historyEnabled = true;
      displayed.busy = false;
    }
  });
  session.beginReset();
  session.finishReset();
  finish();
  await completion;
  assert.deepEqual(displayed, { bookmarks: ["new"], visits: ["new"], historyEnabled: false, busy: true });
  // The newer session still accepts its own completions.
  const current = session.snapshot();
  assert.equal(session.accepts(current), true);
  displayed.busy = false;
  assert.equal(displayed.busy, false);
});

test("stale errors and finalizers cannot overwrite a newer privacy session", async () => {
  const session = new ReaderPersonalDataSession();
  const old = session.snapshot();
  let fail;
  const pending = new Promise((_, reject) => { fail = reject; });
  const state = { error: null, busy: true };
  const completion = pending.catch(() => {
    if (session.accepts(old)) state.error = "Old request failed";
  }).finally(() => {
    if (session.accepts(old)) state.busy = false;
  });
  session.beginReset();
  session.finishReset();
  fail(new Error("stale IPC"));
  await completion;
  assert.deepEqual(state, { error: null, busy: true });
});

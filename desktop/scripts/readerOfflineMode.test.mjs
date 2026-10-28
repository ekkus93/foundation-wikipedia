import test from "node:test";
import assert from "node:assert/strict";
import { ReaderOfflineMode } from "../src/readerOfflineMode.ts";
import { RequestEpoch } from "../src/requestEpoch.ts";

test("offline mode is shared by existing and newly mounted tabs", () => {
  const mode = new ReaderOfflineMode();
  const states = [];
  const unsubA = mode.subscribe(() => states.push(["A", mode.getSnapshot()]));
  const unsubB = mode.subscribe(() => states.push(["B", mode.getSnapshot()]));
  assert.equal(mode.getSnapshot(), false);
  mode.setOffline(true);
  assert.deepEqual(states, [["A", true], ["B", true]]);
  assert.equal(mode.getSnapshot(), true, "new tabs inherit the current mode");
  mode.setOffline(true);
  assert.equal(states.length, 2, "identical changes do not dispatch again");
  unsubA();
  mode.setOffline(false);
  assert.deepEqual(states.at(-1), ["B", false]);
  unsubB();
});

test("global offline transition invalidates hidden-tab request epochs", () => {
  const mode = new ReaderOfflineMode();
  const first = new RequestEpoch();
  const hidden = new RequestEpoch();
  mode.subscribe(() => { if (mode.getSnapshot()) first.invalidate(); });
  mode.subscribe(() => { if (mode.getSnapshot()) hidden.invalidate(); });
  const a = first.begin();
  const b = hidden.begin();
  mode.setOffline(true);
  assert.equal(first.matches(a), false);
  assert.equal(hidden.matches(b), false);
  assert.equal(mode.getSnapshot(), true);
  mode.setOffline(false);
  assert.equal(first.matches(a), false, "reconnect never revives a stale result");
  assert.equal(hidden.matches(b), false);
});

import test from "node:test";
import assert from "node:assert/strict";
import { RequestEpoch } from "../src/requestEpoch.ts";

test("late online result is invalid after disconnect", () => {
  const requests = new RequestEpoch();
  const search = requests.begin();
  assert.equal(requests.matches(search), true);
  requests.invalidate();
  assert.equal(requests.matches(search), false);
});

test("a late old request cannot finish a newer request", () => {
  const requests = new RequestEpoch();
  const first = requests.begin();
  const second = requests.begin();
  assert.equal(requests.matches(first), false);
  assert.equal(requests.matches(second), true);
  requests.invalidate();
  assert.equal(requests.matches(second), false);
  const offline = requests.begin();
  assert.equal(requests.matches(offline), true);
});


test("manual offline transition cancels pending search or article results", () => {
  const requests = new RequestEpoch();
  const search = requests.begin();
  requests.invalidate(); // User checks Work offline while search is pending.
  assert.equal(requests.matches(search), false);
  const article = requests.begin(); // A new online action after leaving offline mode.
  assert.equal(requests.matches(article), true);
  assert.equal(requests.matches(search), false);
  requests.invalidate(); // Disconnect or another explicit offline transition.
  assert.equal(requests.matches(article), false);
});

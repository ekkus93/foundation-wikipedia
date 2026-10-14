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

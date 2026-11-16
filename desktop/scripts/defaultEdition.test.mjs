import test from "node:test";
import assert from "node:assert/strict";
import { isEdition, loadDefaultEdition } from "../src/defaultEdition.ts";

test("only supported editions can become the default", () => {
  assert.equal(isEdition("frwiki"), true);
  for (const bad of ["xxwiki", "", null, 1, "enwiki "]) assert.equal(isEdition(bad), false);
  // Without browser storage (Node), the reader falls back to English.
  assert.equal(loadDefaultEdition(), "enwiki");
});

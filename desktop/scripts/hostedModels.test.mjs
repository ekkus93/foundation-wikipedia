import test from "node:test";
import assert from "node:assert/strict";
import { parseHostedModels, validModelName } from "../src/hostedModels.ts";

test("saved hosted model names are validated and malformed entries ignored", () => {
  assert.deepEqual(parseHostedModels('{"anthropic":"claude-x","openai":"gpt-y"}'), { anthropic: "claude-x", openai: "gpt-y" });
  assert.deepEqual(parseHostedModels('{"anthropic":"has space","openai":42,"other":"x"}'), {});
  for (const raw of [null, "", "not json", "[]", "null", '"x"']) assert.deepEqual(parseHostedModels(raw), {});
  assert.equal(validModelName("claude-sonnet-4-5"), true);
  assert.equal(validModelName("x".repeat(129)), false);
  assert.equal(validModelName("a‮b"), false);
  assert.equal(validModelName(""), false);
});

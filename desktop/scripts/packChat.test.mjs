import test from "node:test";
import assert from "node:assert/strict";
import { numberCitations, parseChatAnswer, parseChatPreview, parseChatProgress } from "../src/packChat.ts";

const passage = (n) => ({ block_id: `wkb:enwiki:2:7002:b${n}`, heading_path: [], excerpt: `Passage ${n}` });

test("previews are validated before they are shown", () => {
  const preview = parseChatPreview({
    id: 1, destination: "http://127.0.0.1:11434", locality: "this device", model: "m", profile: "Default",
    article_title: "Tide", revision_id: 7002, passages: [passage(0)], selected_passage: null,
  });
  assert.equal(preview.passages[0].blockId, "wkb:enwiki:2:7002:b0");
  for (const bad of [
    { locality: "cloudish" }, { passages: "x" }, { id: -1 },
    { passages: [{ block_id: "javascript:x", heading_path: [], excerpt: "" }] },
  ]) {
    assert.throws(() => parseChatPreview({
      id: 1, destination: "d", locality: "this device", model: "m", profile: "p",
      article_title: "t", revision_id: 1, passages: [], selected_passage: null, ...bad,
    }));
  }
});

test("answers keep citations as validated passages, numbered by first appearance", () => {
  const answer = parseChatAnswer({ kind: "supported", parts: [
    { kind: "text", text: "Tides follow the Moon" }, { kind: "citation", passage: passage(0) },
    { kind: "text", text: " and the Sun" }, { kind: "citation", passage: passage(1) },
    { kind: "citation", passage: passage(0) },
  ] });
  const { segments, citations } = numberCitations(answer.parts);
  assert.deepEqual(citations.map((c) => [c.number, c.passage.blockId]),
    [[1, "wkb:enwiki:2:7002:b0"], [2, "wkb:enwiki:2:7002:b1"]]);
  assert.deepEqual(segments.filter((s) => s.kind === "citation").map((s) => s.number), [1, 2, 1]);
  assert.deepEqual(parseChatAnswer({ kind: "insufficient_evidence" }), { kind: "insufficient_evidence" });
  assert.throws(() => parseChatAnswer({ kind: "supported", parts: [{ kind: "html", text: "<b>" }] }));
  assert.throws(() => parseChatAnswer(null));
});

test("malformed progress events are ignored", () => {
  assert.equal(parseChatProgress({ id: 1 }), null);
  assert.deepEqual(parseChatProgress({ id: 2, elapsed_ms: 10, first_output_ms: null, finished: false, draft: "Tides […]" }),
    { id: 2, elapsedMs: 10, firstOutputMs: null, finished: false, draft: "Tides […]" });
});

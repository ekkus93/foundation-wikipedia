import test from "node:test";
import assert from "node:assert/strict";
import { parseReaderChatThreads, parseReaderChatExchanges, ReaderChatDeleteFence, ReaderChatExchangeReadFence, ReaderChatThreadListFence } from "../src/readerChatHistory.ts";

test("thread IDs must be unique, bounded and exactly representable", () => {
  assert.deepEqual(parseReaderChatThreads([2, 1]), [2, 1]);
  assert.deepEqual(parseReaderChatThreads([]), []);
  for (const bad of [null, {}, [0], [-1], [1.5], ["1"], [1, 1],
    [Number.MAX_SAFE_INTEGER + 1], new Array(51).fill(1), Array(1)]) {
    assert.throws(() => parseReaderChatThreads(bad), /Invalid saved chat/);
  }
});

test("only bounded, chronological, complete question-answer pairs are accepted", () => {
  const rows = [[2, 3, "What is gravity?", "An attraction."], [8, 9, "Follow-up", "Another answer."]];
  assert.deepEqual(parseReaderChatExchanges(rows), rows);
  assert.deepEqual(parseReaderChatExchanges([]), []);
  for (const bad of [
    null, {}, Array(1), new Array(21).fill(rows[0]),
    [[1, 2, "question"]], [[1, 2, "question", "answer", "extra"]],
    [[1, 1, "question", "answer"]], [[2, 1, "question", "answer"]],
    [[1.5, 2, "question", "answer"]], [[1, Number.MAX_SAFE_INTEGER + 1, "question", "answer"]],
    [[1, 2, "", "answer"]], [[1, 2, "question", "  "]],
    [[1, 2, "question", "answer"], [2, 3, "question", "answer"]],
    [[1, 2, "question", "answer"], [3, 4, "question", "answer"], [2, 5, "question", "answer"]],
    [[1, 2, "x".repeat(65537), "answer"]],
    [[1, 2, "question", "\ud800"]],
    [[1, 2, "\udc00", "answer"]],
  ]) assert.throws(() => parseReaderChatExchanges(bad), /Invalid saved chat/);
});

test("plain text including markup is preserved, never interpreted as verified citations", () => {
  const rows = [[1, 2, "What is <b>math</b>?", "<img src=x onerror=alert(1)> [fabricated citation]"]];
  assert.deepEqual(parseReaderChatExchanges(rows), rows);
  assert.deepEqual(parseReaderChatExchanges([[1, 2, "質問", "日本語の回答"]]), [[1, 2, "質問", "日本語の回答"]]);
});

test("a delayed deletion cannot mutate a new article, a reused thread ID, or a remounted panel", () => {
  const fence = new ReaderChatDeleteFence();
  const first = { project: "enwiki", pageId: 42, revisionId: 7 };
  const nextRevision = { ...first, revisionId: 8 };
  const otherProject = { ...first, project: "frwiki" };
  const pending = fence.begin(first, 5);
  assert.equal(fence.isCurrent(pending, first, 5), true);
  assert.equal(fence.isCurrent(pending, first, 6), false);
  assert.equal(fence.isCurrent(pending, nextRevision, 5), false);
  assert.equal(fence.isCurrent(pending, otherProject, 5), false);
  fence.invalidate();
  assert.equal(fence.isCurrent(pending, first, 5), false);
  const replacement = fence.begin(first, 5);
  assert.equal(fence.isCurrent(pending, first, 5), false);
  assert.equal(fence.isCurrent(replacement, first, 5), true);
  fence.invalidate();
  assert.equal(fence.isCurrent(replacement, first, 5), false);
});

test("late transcript reads cannot display a previous thread after a new selection", () => {
  const fence = new ReaderChatExchangeReadFence();
  const article = { project: "enwiki", pageId: 42, revisionId: 7 };
  const old = fence.begin(article, 11);
  assert.equal(fence.isCurrent(old, article, 11), true);
  assert.equal(fence.isCurrent(old, article, 12), false);
  assert.equal(fence.isCurrent(old, { ...article, revisionId: 8 }, 11), false);
  const newer = fence.begin(article, 12);
  assert.equal(fence.isCurrent(old, article, 11), false);
  assert.equal(fence.isCurrent(newer, article, 12), true);
  fence.invalidate();
  assert.equal(fence.isCurrent(newer, article, 12), false);
  const repeated = fence.begin(article, 12);
  assert.equal(fence.isCurrent(newer, article, 12), false);
  assert.equal(fence.isCurrent(repeated, article, 12), true);
});

test("late thread-list responses cannot select a conversation from another article", () => {
  const fence = new ReaderChatThreadListFence();
  const article = { project: "enwiki", pageId: 42, revisionId: 7 };
  const old = fence.begin(article);
  assert.equal(fence.isCurrent(old, article), true);
  // A navigation render changes currentArticleRef before effect cleanup.
  assert.equal(fence.isCurrent(old, { ...article, pageId: 43 }), false);
  assert.equal(fence.isCurrent(old, { ...article, revisionId: 8 }), false);
  assert.equal(fence.isCurrent(old, { ...article, project: "frwiki" }), false);
  fence.invalidate();
  assert.equal(fence.isCurrent(old, article), false);
  const newer = fence.begin(article);
  assert.equal(fence.isCurrent(old, article), false);
  assert.equal(fence.isCurrent(newer, article), true);
  // Same identity after remount is still a distinct request.
  const repeated = fence.begin(article);
  assert.equal(fence.isCurrent(newer, article), false);
  assert.equal(fence.isCurrent(repeated, article), true);
  fence.invalidate();
  assert.equal(fence.isCurrent(repeated, article), false);
});

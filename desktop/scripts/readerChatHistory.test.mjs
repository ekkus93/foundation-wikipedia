import test from "node:test";
import assert from "node:assert/strict";
import { parseReaderChatThreads, parseReaderChatExchanges } from "../src/readerChatHistory.ts";

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

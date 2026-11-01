import test from "node:test";
import assert from "node:assert/strict";
import { clampReaderChatPosition } from "../src/readerChatWindow.ts";

test("desktop chat moves freely within the viewport and stops at every edge", () => {
  const clamp = (x, y) => clampReaderChatPosition(x, y, 400, 300, 800, 600);
  assert.deepEqual(clamp(200, 100), { left: 200, top: 100 });
  assert.deepEqual(clamp(-200, -100), { left: 8, top: 8 });
  assert.deepEqual(clamp(900, 900), { left: 392, top: 292 });
  assert.deepEqual(clamp(400, 0), { left: 392, top: 8 });
});

test("viewport shrink and resize never strand the chat outside visible bounds", () => {
  assert.deepEqual(clampReaderChatPosition(500, 400, 350, 250, 480, 360),
    { left: 122, top: 102 });
  assert.deepEqual(clampReaderChatPosition(200, 100, 600, 400, 320, 240),
    { left: 0, top: 0 });
  assert.deepEqual(clampReaderChatPosition(0, 0, 304, 224, 320, 240),
    { left: 8, top: 8 });
});

test("invalid and nonfinite geometry is rejected", () => {
  for (const args of [
    [NaN, 0, 400, 300, 800, 600],
    [0, Infinity, 400, 300, 800, 600],
    [0, 0, 0, 300, 800, 600],
    [0, 0, 400, 300, -1, 600],
  ]) assert.throws(() => clampReaderChatPosition(...args), /Invalid chat window geometry/);
});

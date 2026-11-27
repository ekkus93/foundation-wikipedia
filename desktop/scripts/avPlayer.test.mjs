import test from "node:test";
import assert from "node:assert/strict";
import {
  AV_STREAM_DISCLOSURE, disclosureAccepted, encodeDisclosure, formatClock, isStreamAddress,
  loadDisclosureAccepted, nextCandidate, parseAvStreamInfo, positionFraction, saveDisclosureAccepted,
} from "../src/avPlayer.ts";

test("the clock shows m:ss and h:mm:ss, and 0:00 for unknown times", () => {
  assert.equal(formatClock(0), "0:00");
  assert.equal(formatClock(5.9), "0:05");
  assert.equal(formatClock(65), "1:05");
  assert.equal(formatClock(600), "10:00");
  assert.equal(formatClock(3600), "1:00:00");
  assert.equal(formatClock(3725), "1:02:05");
  for (const bad of [NaN, Infinity, -4]) assert.equal(formatClock(bad), "0:00");
});

test("a failed candidate gives way to the next until none is left", () => {
  assert.equal(nextCandidate(0, 5), 1);
  assert.equal(nextCandidate(3, 5), 4);
  assert.equal(nextCandidate(4, 5), null);
  assert.equal(nextCandidate(0, 1), null);
  assert.equal(nextCandidate(0, 0), null);
});

test("the position fraction is clamped and safe for unknown lengths", () => {
  assert.equal(positionFraction(30, 60), 0.5);
  assert.equal(positionFraction(90, 60), 1);
  assert.equal(positionFraction(-1, 60), 0);
  for (const d of [0, NaN, Infinity, -5]) assert.equal(positionFraction(10, d), 0);
});

test("the disclosure choice encodes as v1:accepted and nothing else counts", () => {
  assert.equal(encodeDisclosure(), "v1:accepted");
  assert.equal(disclosureAccepted(encodeDisclosure()), true);
  for (const bad of [null, undefined, "", "accepted", "v2:accepted", "v1:accepted "]) {
    assert.equal(disclosureAccepted(bad), false);
  }
  // Without browser storage (Node), it is "not yet" and saving reports failure.
  assert.equal(loadDisclosureAccepted(), false);
  assert.equal(saveDisclosureAccepted(), false);
  assert.match(AV_STREAM_DISCLOSURE, /upload\.wikimedia\.org/);
});

test("only plain https addresses on the media host reach the player", () => {
  assert.equal(isStreamAddress("https://upload.wikimedia.org/wikipedia/commons/2/24/A.webm"), true);
  for (const bad of [
    "http://upload.wikimedia.org/a.webm", "https://example.org/a.webm", "https://upload.wikimedia.org:8443/a",
    "https://u@upload.wikimedia.org/a", "https://upload.wikimedia.org/a?x=1", "https://upload.wikimedia.org/a#t",
    "https://upload.wikimedia.org.evil.example/a", "javascript:alert(1)", "nope",
  ]) assert.equal(isStreamAddress(bad), false, bad);
});

test("the command's answer is checked before use", () => {
  const good = { title: "A b.webm", video: true, candidates: ["https://upload.wikimedia.org/x"] };
  assert.deepEqual(parseAvStreamInfo(good), good);
  for (const bad of [
    null, 1, {}, { ...good, candidates: [] }, { ...good, video: "yes" }, { ...good, title: 1 },
    { ...good, candidates: ["https://example.org/x"] }, { ...good, candidates: [3] },
  ]) assert.equal(parseAvStreamInfo(bad), null);
});

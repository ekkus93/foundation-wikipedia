import test from "node:test";
import assert from "node:assert/strict";
import { RequestEpoch } from "../src/requestEpoch.ts";
import {
  appendReaderHistory, backReaderArticle, emptyReaderNavigation,
  forwardReaderArticle, mountedReaderFrames, openBookmarkedReaderArticle, openReaderArticle, popReaderHistory,
} from "../src/readerHistory.ts";

const article = (pageId, revisionId = pageId * 10, project = "enwiki") => ({
  project, pageId, revisionId, title: String(pageId), html: "<p>" + pageId + "</p>",
});

test("Back and Forward preserve exact revisions without network refetch", () => {
  const first = article(1, 101);
  const second = article(2, 205);
  let state = openReaderArticle(emptyReaderNavigation(), first);
  state = openReaderArticle(state, second);
  state = backReaderArticle(state);
  assert.deepEqual(state.current, first);
  assert.deepEqual(state.forward, [second]);
  state = forwardReaderArticle(state);
  assert.deepEqual(state.current, second);
  assert.deepEqual(state.back, [first]);
});

test("Back from the first article returns to search with Forward available", () => {
  const first = article(1);
  let state = backReaderArticle(openReaderArticle(emptyReaderNavigation(), first));
  assert.equal(state.current, null);
  assert.deepEqual(state.forward, [first]);
  state = forwardReaderArticle(state);
  assert.deepEqual(state.current, first);
  assert.equal(state.forward.length, 0);
});

test("Opening a different article clears stale forward history", () => {
  let state = openReaderArticle(emptyReaderNavigation(), article(1));
  state = openReaderArticle(state, article(2));
  state = backReaderArticle(state);
  state = openReaderArticle(state, article(3));
  assert.equal(state.current.pageId, 3);
  assert.deepEqual(state.forward, []);
  assert.equal(forwardReaderArticle(state), state);
});

test("empty navigation and unavailable Back are harmless", () => {
  const state = emptyReaderNavigation();
  assert.equal(backReaderArticle(state), state);
  assert.equal(forwardReaderArticle(state), state);
  assert.deepEqual(popReaderHistory([]), { previous: undefined, rest: [] });
});

test("history retains only the 12 newest snapshots", () => {
  let stack = [];
  for (let id = 1; id <= 60; id++) stack = appendReaderHistory(stack, article(id));
  assert.equal(stack.length, 12);
  assert.equal(stack[0].pageId, 49);
  assert.equal(popReaderHistory(stack).previous.pageId, 60);
});

test("history drops old snapshots before exceeding the HTML memory budget", () => {
  const entry = (id, size) => ({ id, html: "x".repeat(size) });
  const stack = appendReaderHistory(appendReaderHistory([], entry(1, 3_000_000)), entry(2, 2_000_000));
  assert.deepEqual(stack.map((item) => item.id), [2]);
  assert.deepEqual(appendReaderHistory(stack, entry(3, 4_000_001)), []);
});

test("nearby revision frame identities survive Back, Forward and search-result return", () => {
  const withFrame = (id) => ({ ...article(id, id * 100), frameId: id });
  let state = openReaderArticle(emptyReaderNavigation(), withFrame(1));
  state = openReaderArticle(state, withFrame(2));
  state = openReaderArticle(state, withFrame(3));
  state = openReaderArticle(state, withFrame(4));
  assert.deepEqual(mountedReaderFrames(state).map((a) => a.frameId), [2, 3, 4]);
  state = backReaderArticle(state);
  assert.deepEqual(mountedReaderFrames(state).map((a) => a.frameId), [1, 2, 3, 4]);
  state = forwardReaderArticle(state);
  assert.deepEqual(mountedReaderFrames(state).map((a) => a.frameId), [2, 3, 4]);
  const first = backReaderArticle(openReaderArticle(emptyReaderNavigation(), withFrame(5)));
  assert.equal(first.current, null);
  assert.deepEqual(mountedReaderFrames(first).map((a) => a.frameId), [5]);
  assert.deepEqual(mountedReaderFrames(emptyReaderNavigation()), []);
});

test("offline transition invalidates an in-flight fetch without discarding loaded revisions", () => {
  const first = article(11, 1101);
  const second = article(22, 2202);
  const loaded = openReaderArticle(openReaderArticle(emptyReaderNavigation(), first), second);
  const epoch = new RequestEpoch();
  const inFlight = epoch.begin();
  // A network-loss event invalidates responses but does not erase in-memory
  // snapshots. Those snapshots remain text-only, not installed offline media.
  epoch.invalidate();
  assert.equal(epoch.matches(inFlight), false);
  assert.deepEqual(loaded.current, second);
  const back = backReaderArticle(loaded);
  assert.deepEqual(back.current, first);
  assert.deepEqual(forwardReaderArticle(back).current, second);
});

test("bookmark navigation preserves same-edition Back but isolates another edition", () => {
  const first = article(12, 120);
  const latest = article(12, 121);
  const old = openReaderArticle(emptyReaderNavigation(), first);
  const sameEdition = openBookmarkedReaderArticle(old, latest);
  assert.deepEqual(sameEdition.current, latest);
  assert.deepEqual(backReaderArticle(sameEdition).current, first);
  const german = article(12, 121, "dewiki");
  const otherEdition = openBookmarkedReaderArticle(old, german);
  assert.deepEqual(otherEdition.current, german);
  assert.deepEqual(otherEdition.back, []);
  assert.deepEqual(otherEdition.forward, []);
  assert.equal(backReaderArticle(otherEdition).current, null);
});


test("saved article provenance, not the selected edition, isolates Back history", () => {
  const english = article(42, 333, "enwiki");
  const german = article(42, 333, "dewiki");
  const state = openReaderArticle(emptyReaderNavigation(), english);
  // Identical numeric page/revision IDs in different editions are not the
  // same article. A UI edition switch cannot rebind a mounted revision.
  const switched = openBookmarkedReaderArticle(state, german);
  assert.deepEqual(switched.current, german);
  assert.equal(switched.back.length, 0);
  assert.equal(backReaderArticle(switched).current, null);
  const reopened = openBookmarkedReaderArticle(state, article(42, 334, "enwiki"));
  assert.equal(backReaderArticle(reopened).current.project, "enwiki");
  assert.equal(backReaderArticle(reopened).current.revisionId, 333);
});

test("ordinary navigation cannot carry Back history into another edition", () => {
  const english = article(42, 333, "enwiki");
  const german = article(42, 333, "dewiki");
  const englishNext = article(43, 334, "enwiki");
  let state = openReaderArticle(emptyReaderNavigation(), english);
  state = openReaderArticle(state, englishNext);
  assert.deepEqual(backReaderArticle(state).current, english);

  // An asynchronous title/link response may carry an edition different from
  // the current snapshot; identical numeric IDs must not merge histories.
  state = openReaderArticle(state, german);
  assert.deepEqual(state.current, german);
  assert.deepEqual(state.back, []);
  assert.deepEqual(state.forward, []);
  assert.equal(backReaderArticle(state).current, null);

  const germanNext = article(43, 334, "dewiki");
  const sameEdition = openReaderArticle(state, germanNext);
  assert.deepEqual(backReaderArticle(sameEdition).current, german);
});

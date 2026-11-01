import test from "node:test";
import assert from "node:assert/strict";
import { parseAiLocalInventory, aiLocalityDisclosure, validAiLocalModelId, parseAiSavedLocalProfile, parseAiSavedLocalProfiles, validAiLocalProfileName, sameAiSavedLocalProfiles } from "../src/aiLocalModels.ts";

const endpoint = "http://127.0.0.1:11434";
const protocol = "ollama";
const response = () => ({
  endpoint, protocol, locality: "localhost",
  models: [{ id: "qwen3:8b", contextWindow: null }, { id: "modèle-1", contextWindow: 4096 }],
});

test("exact requested provider, endpoint, model, and locale are retained", () => {
  assert.deepEqual(parseAiLocalInventory(response(), protocol, endpoint), response());
  assert.match(aiLocalityDisclosure("local-network"), /not.*offline|unavailable offline/i);
  assert.match(aiLocalityDisclosure("localhost"), /not.*on-device/i);
});

test("rejects substituted endpoints, cloud locality, mismatched protocol, and invalid payloads", () => {
  const variants = [null, [], {}, { ...response(), endpoint: "https://example.com" },
    { ...response(), protocol: "cloud" }, { ...response(), locality: "cloud" },
    { ...response(), models: {} }, { ...response(), models: Array(257).fill({ id: "a", contextWindow: null }) }];
  for (const value of variants) assert.throws(() => parseAiLocalInventory(value, protocol, endpoint), /Invalid local model inventory/);
});

test("rejects duplicate, spoofed and malformed model names and out-of-range context counts", () => {
  for (const models of [
    [{id: "a", contextWindow: null}, {id: "a", contextWindow: null}],
    [{id: "a\u202eb", contextWindow: null}],
    [{id: "a\u200bb", contextWindow: null}],
    [{id: "a", contextWindow: 0}],
    [{id: "a", contextWindow: -1}],
    [{id: "a", contextWindow: 2 ** 32}],
    [{id: "a", contextWindow: "512"}],
    [{id: "a"}],
  ]) {
    assert.throws(() => parseAiLocalInventory({ ...response(), models }, protocol, endpoint), /Invalid local model inventory/);
  }
  assert.equal(validAiLocalModelId("qwen3:8b"), true);
  assert.equal(validAiLocalModelId("modèle-1"), true);
  assert.equal(validAiLocalModelId("qwen\u{e0100}3"), false);
  assert.equal(validAiLocalModelId("a\ud800b"), false);
  assert.equal(validAiLocalModelId("a\udc00b"), false);
  assert.equal(validAiLocalModelId("a🐈"), true);
  assert.equal(validAiLocalModelId("a".repeat(257)), false);
});


test("saved profile shape is non-secret and preserves exact model hint", () => {
  const profile = { protocol: "ollama", endpoint, model: "qwen3:8b" };
  assert.deepEqual(parseAiSavedLocalProfile(profile), profile);
  assert.equal(parseAiSavedLocalProfile(null), null);
  for (const bad of [
    {}, { ...profile, apiKey: "secret" },
    { ...profile, protocol: "cloud" },
    { ...profile, endpoint: "https://127.0.0.1:11434" },
    { ...profile, endpoint: "http://user:secret@127.0.0.1" },
    { ...profile, model: "bad\u202emodel" },
    { ...profile, model: "" },
  ]) assert.throws(() => parseAiSavedLocalProfile(bad), /Invalid saved AI profile/);
});


test("bounded named local profiles reject secret-bearing rows and duplicate names", () => {
  const profile = { name: "Desktop", protocol: "ollama", endpoint, model: "qwen3:8b" };
  const collection = { active: "Desktop", profiles: [profile] };
  assert.deepEqual(parseAiSavedLocalProfiles(collection), collection);
  assert.deepEqual(parseAiSavedLocalProfiles({ active: null, profiles: [] }), { active: null, profiles: [] });
  assert.ok(validAiLocalProfileName("LAN Office"));
  for (const name of ["", " space", "bad/name", "bad\u202e", "a".repeat(41)]) {
    assert.equal(validAiLocalProfileName(name), false);
  }
  for (const invalid of [
    null, {}, { active: "Missing", profiles: [profile] },
    { active: null, profiles: [profile] },
    { active: "Desktop", profiles: [{ ...profile, apiKey: "secret" }] },
    { active: "Desktop", profiles: [profile, { ...profile, name: "desktop" }] },
    { active: null, profiles: Array(9).fill(profile) },
    { active: "Desktop", profiles: [{ ...profile, endpoint: "https://public.example" }] },
  ]) assert.throws(() => parseAiSavedLocalProfiles(invalid), /Invalid saved AI profiles/);
});


test("profile focus reconciliation detects remote mutations without changing local drafts", () => {
  const original = { active: "Desktop", profiles: [{ name: "Desktop", protocol, endpoint, model: "qwen3:8b" }] };
  const equivalent = parseAiSavedLocalProfiles(JSON.parse(JSON.stringify(original)));
  assert.equal(sameAiSavedLocalProfiles(original, equivalent), true);
  assert.equal(sameAiSavedLocalProfiles(original, { ...original, active: null }), false);
  assert.equal(sameAiSavedLocalProfiles(original, { ...original, profiles: [{ ...original.profiles[0], model: "other" }] }), false);
  assert.equal(sameAiSavedLocalProfiles(original, { ...original, profiles: [...original.profiles, { ...original.profiles[0], name: "LAN" }] }), false);
  assert.equal(sameAiSavedLocalProfiles(original, { ...original, profiles: [] }), false);
});

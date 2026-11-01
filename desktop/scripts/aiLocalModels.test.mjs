import test from "node:test";
import assert from "node:assert/strict";
import { parseAiLocalInventory, aiLocalityDisclosure, validAiLocalModelId, parseAiSavedLocalProfile, parseAiSavedLocalProfiles, validAiLocalProfileName, sameAiSavedLocalProfiles, needsAiProfileReview, classifyAiLocalEndpoint, AiProfileRefreshGate, shouldPollAiProfiles, canSaveAiLocalChoice, canTestAiLocalChoice, canConfirmAiLocalProfileDelete, latestAiSavedLocalDraft, activeAiLocalProfileDestination } from "../src/aiLocalModels.ts";

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
  assert.deepEqual(parseAiSavedLocalProfiles(collection), { ...collection, revision: 0 });
  assert.deepEqual(parseAiSavedLocalProfiles({ ...collection, revision: 7 }), { ...collection, revision: 7 });
  assert.deepEqual(parseAiSavedLocalProfiles({ active: null, profiles: [] }), { revision: 0, active: null, profiles: [] });
  assert.deepEqual(parseAiSavedLocalProfiles({ active: null, profiles: [profile], revision: 4 }), {
    active: null, profiles: [profile], revision: 4,
  }, "saved profiles may be present while the provider is disabled");
  assert.ok(validAiLocalProfileName("LAN Office"));
  for (const name of ["", " space", "bad/name", "bad\u202e", "a".repeat(41)]) {
    assert.equal(validAiLocalProfileName(name), false);
  }
  for (const invalid of [
    null, {}, { active: "Missing", profiles: [profile] },
    { active: "unknown", profiles: [profile] },
    { active: "Desktop", profiles: [{ ...profile, apiKey: "secret" }] },
    { active: "Desktop", profiles: [profile, { ...profile, name: "desktop" }] },
    { active: null, profiles: Array(9).fill(profile) },
    { ...collection, revision: -1 },
    { ...collection, revision: 1.5 },
    { ...collection, revision: Number.MAX_SAFE_INTEGER + 1 },
    { ...collection, revision: "1" },
    { active: "Desktop", profiles: [{ ...profile, endpoint: "https://public.example" }] },
  ]) assert.throws(() => parseAiSavedLocalProfiles(invalid), /Invalid saved AI profiles/);
});


test("profile focus reconciliation detects remote mutations without changing local drafts", () => {
  const original = { revision: 0, active: "Desktop", profiles: [{ name: "Desktop", protocol, endpoint, model: "qwen3:8b" }] };
  const equivalent = parseAiSavedLocalProfiles(JSON.parse(JSON.stringify(original)));
  assert.equal(sameAiSavedLocalProfiles(original, equivalent), true);
  assert.equal(sameAiSavedLocalProfiles(original, { ...original, active: null }), false);
  assert.equal(sameAiSavedLocalProfiles(original, { ...original, profiles: [{ ...original.profiles[0], model: "other" }] }), false);
  assert.equal(sameAiSavedLocalProfiles(original, { ...original, profiles: [...original.profiles, { ...original.profiles[0], name: "LAN" }] }), false);
  assert.equal(sameAiSavedLocalProfiles(original, { ...original, profiles: [] }), false);
});

test("local endpoint classification matches native locality for accepted HTTP addresses", () => {
  for (const [url, locality] of [
    ["http://localhost:11434", "localhost"],
    ["http://LOCALHOST/v1", "localhost"],
    ["http://127.2.3.4", "localhost"],
    ["http://[::1]:11434/v1", "localhost"],
    ["http://10.1.2.3:11434", "local-network"],
    ["http://172.31.1.1/v1", "local-network"],
    ["http://192.168.0.50:8080/v1/", "local-network"],
    ["http://[fd00::42]:11434", "local-network"],
  ]) assert.equal(classifyAiLocalEndpoint(url), locality, url);
  for (const url of [
    "https://127.0.0.1", "http://example.com", "http://ollama.lan",
    "http://localhost.evil", "http://8.8.8.8", "http://169.254.169.254",
    "http://0.0.0.0", "http://172.32.0.1", "http://[fe80::1]",
    "http://[2001:4860::8888]", "http://[::ffff:127.0.0.1]",
    "http://localhost:0", "http://localhost:65536",
    "http://localhost:abc", "http://localhost:11434/api",
    "http://localhost:11434/v1?key=secret", "http://user:pass@localhost",
    "http://127.0.0.1:11434/%2e%2e", "http://127.0.0.01",
  ]) assert.equal(classifyAiLocalEndpoint(url), null, url);
});

test("forged public saved endpoints and inventory locality fail closed in UI", () => {
  const profile = { protocol, endpoint: "http://example.com", model: "qwen3:8b" };
  assert.throws(() => parseAiSavedLocalProfile(profile), /Invalid saved AI profile/);
  assert.throws(() => parseAiSavedLocalProfiles({
    active: "Public", profiles: [{ ...profile, name: "Public" }],
  }), /Invalid saved AI profiles/);
  assert.throws(() => parseAiLocalInventory({
    ...response(), locality: "local-network",
  }, protocol, endpoint), /Invalid local model inventory/);
  const lan = "http://192.168.1.10:11434";
  assert.throws(() => parseAiLocalInventory({
    ...response(), endpoint: lan, locality: "localhost",
  }, protocol, lan), /Invalid local model inventory/);
});

test("profile review fence stays pending after subsequent unchanged refreshes", () => {
  const old = { active: "Desktop", profiles: [{ name: "Desktop", protocol, endpoint, model: "old" }] };
  const current = { active: "Desktop", profiles: [{ ...old.profiles[0], model: "new" }] };
  assert.equal(needsAiProfileReview(old, old), false);
  assert.equal(needsAiProfileReview(old, current), true);
  assert.equal(needsAiProfileReview(current, current, true), true);
  assert.equal(needsAiProfileReview(current, current, false), false);
  assert.equal(needsAiProfileReview(old, { ...old, active: null }), true);
});

test("profile snapshot generation detects ABA even with identical saved fields", () => {
  const initial = parseAiSavedLocalProfiles({ revision: 1, active: "Desktop", profiles: [{ name: "Desktop", protocol, endpoint, model: "a" }] });
  const restored = parseAiSavedLocalProfiles({ ...initial, revision: 3 });
  assert.equal(sameAiSavedLocalProfiles(initial, restored), false);
  assert.equal(needsAiProfileReview(initial, restored), true);
  assert.equal(sameAiSavedLocalProfiles(restored, { ...restored }), true);
});

test("visible cross-process profile refresh is coalesced and hidden windows do not poll", () => {
  assert.equal(shouldPollAiProfiles("visible"), true);
  assert.equal(shouldPollAiProfiles("hidden"), false);
  assert.equal(shouldPollAiProfiles("prerender"), false);
  const gate = new AiProfileRefreshGate();
  assert.equal(gate.begin(), true);
  assert.equal(gate.begin(), false);
  assert.equal(gate.begin(), false);
  assert.equal(gate.finish(), true, "several triggers require only one follow-up");
  assert.equal(gate.begin(), true);
  assert.equal(gate.finish(), false, "no extra refresh without a queued trigger");
  assert.equal(gate.begin(), true, "a completed read releases the gate");
  assert.equal(gate.finish(), false);
});


test("saved local choice must match the exact discovered server and advertised model", () => {
  const inventory = parseAiLocalInventory(response(), protocol, endpoint);
  assert.equal(canSaveAiLocalChoice(inventory, protocol, endpoint, "qwen3:8b"), true);
  assert.equal(canSaveAiLocalChoice(inventory, protocol, endpoint, "modèle-1"), true);
  assert.equal(canSaveAiLocalChoice(null, protocol, endpoint, "qwen3:8b"), false);
  assert.equal(canSaveAiLocalChoice(inventory, protocol, endpoint, ""), false);
  assert.equal(canSaveAiLocalChoice(inventory, protocol, endpoint, "not-advertised"), false);
  assert.equal(canSaveAiLocalChoice(inventory, protocol, endpoint, "qwen\\u202e3:8b"), false);
  assert.equal(canSaveAiLocalChoice(inventory, "openai-compatible", endpoint, "qwen3:8b"), false);
  assert.equal(canSaveAiLocalChoice(inventory, protocol, "http://127.0.0.1:11435", "qwen3:8b"), false);
  assert.equal(canSaveAiLocalChoice(inventory, protocol, "http://8.8.8.8", "qwen3:8b"), false);
  assert.equal(canSaveAiLocalChoice({ ...inventory, locality: "local-network" }, protocol, endpoint, "qwen3:8b"), false);
});


test("saved profile deletion requires a fresh explicit confirmation for the selected name", () => {
  const saved = parseAiSavedLocalProfiles({
    revision: 9, active: "Desktop",
    profiles: [{ name: "Desktop", protocol, endpoint, model: "qwen3:8b" }],
  });
  assert.equal(canConfirmAiLocalProfileDelete(null, "Desktop", saved, false), false);
  assert.equal(canConfirmAiLocalProfileDelete("Desktop", "Desktop", saved, false), true);
  assert.equal(canConfirmAiLocalProfileDelete("Desktop", "Other", saved, false), false);
  assert.equal(canConfirmAiLocalProfileDelete("Other", "Other", saved, false), false);
  assert.equal(canConfirmAiLocalProfileDelete("Desktop", "Desktop", saved, true), false);
  assert.equal(canConfirmAiLocalProfileDelete("Desktop", "Desktop", { ...saved, profiles: [] }, false), false);
});


test("conflict resolution loads current saved values without treating model hints as verified", () => {
  const current = parseAiSavedLocalProfiles({
    revision: 12, active: "LAN",
    profiles: [
      { name: "Desktop", protocol, endpoint, model: "new-local-model" },
      { name: "LAN", protocol: "openai-compatible", endpoint: "http://192.168.1.10:8080/v1", model: "lan-model" },
    ],
  });
  assert.deepEqual(latestAiSavedLocalDraft(current, "Desktop"), {
    name: "Desktop", protocol, endpoint, modelHint: "new-local-model",
  }, "a matching edited profile uses its newest persisted fields");
  assert.deepEqual(latestAiSavedLocalDraft(current, "Deleted"), {
    name: "LAN", protocol: "openai-compatible", endpoint: "http://192.168.1.10:8080/v1", modelHint: "lan-model",
  }, "deleted drafts fall back to the latest active profile");
  assert.deepEqual(latestAiSavedLocalDraft(current, ""), latestAiSavedLocalDraft(current, "Deleted"));
  assert.deepEqual(latestAiSavedLocalDraft({ revision: 13, active: null, profiles: [] }, "Deleted"), {
    name: "", protocol: "ollama", endpoint, modelHint: "",
  }, "an empty saved collection resets to a nonconnected editor");
});

test("saved active AI destination is disclosed independently of an unsaved draft", () => {
  const saved = parseAiSavedLocalProfiles({
    revision: 6, active: "LAN",
    profiles: [
      { name: "Desktop", protocol, endpoint, model: "local-model" },
      { name: "LAN", protocol: "openai-compatible", endpoint: "http://192.168.1.20:8080/v1", model: "remote-lan-model" },
    ],
  });
  assert.deepEqual(activeAiLocalProfileDestination(saved), {
    name: "LAN", protocol: "openai-compatible", endpoint: "http://192.168.1.20:8080/v1", locality: "local-network",
  });
  assert.match(aiLocalityDisclosure(activeAiLocalProfileDestination(saved).locality), /LAN/);
  assert.deepEqual(activeAiLocalProfileDestination({ ...saved, active: "Desktop" }), {
    name: "Desktop", protocol, endpoint, locality: "localhost",
  });
  assert.equal(activeAiLocalProfileDestination({ revision: 7, active: null, profiles: [] }), null);
  assert.equal(activeAiLocalProfileDestination({ ...saved, active: "Missing" }), null);
  assert.equal(activeAiLocalProfileDestination({
    ...saved, profiles: [{ ...saved.profiles[1], endpoint: "https://public.example" }],
  }), null, "invalid persisted destination is never presented as a trusted locality");
});

test("disabled local provider retains profiles but discloses no active destination", () => {
  const disabled = parseAiSavedLocalProfiles({
    revision: 8, active: null,
    profiles: [{ name: "LAN", protocol: "openai-compatible", endpoint: "http://192.168.1.20:8080/v1", model: "model-a" }],
  });
  assert.equal(activeAiLocalProfileDestination(disabled), null);
  assert.equal(latestAiSavedLocalDraft(disabled, "LAN").modelHint, "model-a");
  assert.equal(sameAiSavedLocalProfiles(disabled, { ...disabled, active: "LAN" }), false);
  assert.equal(needsAiProfileReview(disabled, { ...disabled, active: "LAN" }), true);
});

test("explicit inference test requires fresh exact model, network permission and no pending action", () => {
  const inventory = parseAiLocalInventory(response(), protocol, endpoint);
  assert.equal(canTestAiLocalChoice(inventory, protocol, endpoint, "qwen3:8b", false, false), true);
  assert.equal(canTestAiLocalChoice(null, protocol, endpoint, "qwen3:8b", false, false), false);
  assert.equal(canTestAiLocalChoice(inventory, protocol, endpoint, "qwen3:8b", true, false), false);
  assert.equal(canTestAiLocalChoice(inventory, protocol, endpoint, "qwen3:8b", false, true), false);
  assert.equal(canTestAiLocalChoice(inventory, protocol, endpoint, "unadvertised", false, false), false);
  assert.equal(canTestAiLocalChoice(inventory, "openai-compatible", endpoint, "qwen3:8b", false, false), false);
  assert.equal(canTestAiLocalChoice(inventory, protocol, "http://192.168.1.2:11434", "qwen3:8b", false, false), false);
});

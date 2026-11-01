import { useEffect, useId, useRef, useState, useSyncExternalStore, type FormEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { type AiLocalInventory, type AiLocalProtocol, aiLocalityDisclosure, parseAiLocalInventory, parseAiSavedLocalProfiles, validAiLocalProfileName, type AiSavedLocalProfiles, sameAiSavedLocalProfiles, needsAiProfileReview, AiProfileRefreshGate, shouldPollAiProfiles, canSaveAiLocalChoice, canTestAiLocalChoice, canConfirmAiLocalProfileDelete, latestAiSavedLocalDraft, classifyAiLocalEndpoint, activeAiLocalProfileDestination } from "./aiLocalModels";
import { RequestEpoch } from "./requestEpoch";
import { ReaderOfflineMode } from "./readerOfflineMode";

/** Saved model IDs remain untrusted until the user explicitly rediscovers them. */
export default function AiAssistantSettings({ offlineMode }: { offlineMode: ReaderOfflineMode }) {
  const id = useId();
  const [protocol, setProtocol] = useState<AiLocalProtocol>("ollama");
  const [endpoint, setEndpoint] = useState("http://127.0.0.1:11434");
  const [inventory, setInventory] = useState<AiLocalInventory | null>(null);
  const [selectedModel, setSelectedModel] = useState("");
  const [busy, setBusy] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{ ok: boolean; message: string } | null>(null);
  const [savedModelHint, setSavedModelHint] = useState("");
  const [profileName, setProfileName] = useState("Default");
  const [savedProfiles, setSavedProfiles] = useState<AiSavedLocalProfiles>({ revision: 0, active: null, profiles: [] });
  const [saving, setSaving] = useState(false);
  const [savedNotice, setSavedNotice] = useState<string | null>(null);
  // A refreshed CAS snapshot must not silently authorize an old draft to
  // overwrite changes made in another window.
  const [needsReview, setNeedsReview] = useState(false);
  const [deletePending, setDeletePending] = useState<string | null>(null);
  const editRevision = useRef(0);
  const [error, setError] = useState<string | null>(null);
  const epoch = useRef(new RequestEpoch());
  // One generation for initial load, focus/event refresh and conflict recovery.
  // Otherwise a slow initial read can overwrite a newer window notification.
  const profileReads = useRef(new RequestEpoch());
  const activeProbe = useRef<number | null>(null);
  const activeDiscovery = useRef<number | null>(null);

  function cancelActiveDiscovery() {
    const requestId = activeDiscovery.current;
    activeDiscovery.current = null;
    if (requestId !== null) {
      void invoke<void>("cancel_local_ai_model_discovery", { requestId }).catch(() => {});
    }
  }

  function cancelActiveProbe() {
    const requestId = activeProbe.current;
    activeProbe.current = null;
    if (requestId !== null) {
      void invoke<void>("cancel_local_ai_model_test", { requestId }).catch(() => {});
    }
  }
  const offline = useSyncExternalStore(offlineMode.subscribe, offlineMode.getSnapshot, offlineMode.getSnapshot);

  useEffect(() => () => { epoch.current.invalidate(); profileReads.current.invalidate(); cancelActiveDiscovery(); cancelActiveProbe(); }, []);
  useEffect(() => {
    let active = true;
    const revision = editRevision.current;
    const request = profileReads.current.begin();
    void invoke<unknown>("read_local_ai_profiles").then(parseAiSavedLocalProfiles).then((saved) => {
      if (!active || !profileReads.current.matches(request) || revision !== editRevision.current) return;
      setSavedProfiles(saved);
      const profile = saved.profiles.find((item) => item.name === saved.active);
      if (!profile) return;
      setProtocol(profile.protocol);
      setEndpoint(profile.endpoint);
      setSavedModelHint(profile.model);
      setProfileName(profile.name);
      setSavedNotice("Saved local profile loaded. Discover models again to verify availability.");
    }).catch(() => {
      if (active && profileReads.current.matches(request) && revision === editRevision.current) setSavedNotice("Could not restore the saved local profile.");
    });
    return () => { active = false; };
  }, []);
  // Reconcile durable profiles after another window's native save event,
  // window focus or visibility restoration (the latter two cover processes).
  // Keep the unsaved form; all refresh paths read local SQLite only.
  useEffect(() => {
    if (saving) return;
    let active = true;
    const refreshGate = new AiProfileRefreshGate();
    const refreshProfiles = () => {
      // Coalesce event, focus and timer triggers rather than overlapping
      // SQLite reads or starving the latest RequestEpoch on slow disks.
      if (!refreshGate.begin()) return;
      const revision = editRevision.current;
      const request = profileReads.current.begin();
      void invoke<unknown>("read_local_ai_profiles").then(parseAiSavedLocalProfiles).then((current) => {
        if (!active || !profileReads.current.matches(request) || revision !== editRevision.current) return;
        if (!sameAiSavedLocalProfiles(current, savedProfiles)) {
          setSavedProfiles(current);
          setDeletePending(null);
          setNeedsReview((pending) => needsAiProfileReview(savedProfiles, current, pending));
          setSavedNotice("Saved profiles changed in another window. Your draft was kept; acknowledge the new saved state before saving.");
        }
      }).catch(() => {
        // A failed read must not replace a user's draft or erase saved data.
      }).finally(() => {
        if (refreshGate.finish() && active) refreshProfiles();
      });
    };
    const refreshOnVisibility = () => {
      if (shouldPollAiProfiles(document.visibilityState)) refreshProfiles();
    };
    window.addEventListener("focus", refreshProfiles);
    document.addEventListener("visibilitychange", refreshOnVisibility);
    // Another desktop process may update SQLite while both windows remain
    // visible and neither regains focus. This never contacts an AI endpoint.
    const poll = window.setInterval(() => {
      if (shouldPollAiProfiles(document.visibilityState)) refreshProfiles();
    }, 5000);
    // Native events carry no saved-profile data. They only trigger a fresh,
    // validated read; failed registration leaves focus reconciliation intact.
    let unlisten: (() => void) | undefined;
    void listen("ai-local-profiles-changed", refreshProfiles).then((dispose) => {
      if (active) unlisten = dispose;
      else dispose();
    }).catch(() => {});
    return () => {
      active = false;
      window.clearInterval(poll);
      profileReads.current.invalidate();
      unlisten?.();
      window.removeEventListener("focus", refreshProfiles);
      document.removeEventListener("visibilitychange", refreshOnVisibility);
    };
  }, [savedProfiles, saving]);

  // Network permission may be withdrawn during an in-flight discovery.
  useEffect(() => offlineMode.subscribe(() => {
    if (offlineMode.getSnapshot()) {
      cancelActiveDiscovery();
      cancelActiveProbe();
      epoch.current.invalidate();
      setBusy(false);
      setTesting(false);
      setTestResult(null);
      setInventory(null);
      setSelectedModel("");
      setError(null);
    }
  }), [offlineMode]);

  function invalidate(preserveSavedHint = false) {
    cancelActiveDiscovery();
    cancelActiveProbe();
    epoch.current.invalidate();
    profileReads.current.invalidate();
    setBusy(false);
    setTesting(false);
    setTestResult(null);
    setError(null);
    setInventory(null);
    setSelectedModel("");
    if (!preserveSavedHint) setSavedModelHint("");
    setSavedNotice(null);
    setDeletePending(null);
    editRevision.current += 1;
  }

  async function discover(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (busy || testing || offlineMode.getSnapshot()) return;
    invalidate(true);
    if (!endpoint || endpoint.length > 512 || endpoint.trim() !== endpoint) {
      setError("Enter an exact localhost or private-IP URL without spaces.");
      return;
    }
    const request = epoch.current.begin();
    setBusy(true);
    let requestId: number | null = null;
    try {
      // Register a native single-use lease before opening a socket.
      // A late registration is cancelled, never allowed to start a GET.
      requestId = await invoke<number>("begin_local_ai_model_discovery");
      if (!Number.isSafeInteger(requestId) || requestId <= 0) throw new Error("Invalid discovery lease");
      if (!epoch.current.matches(request) || offlineMode.getSnapshot()) {
        void invoke<void>("cancel_local_ai_model_discovery", { requestId }).catch(() => {});
        return;
      }
      activeDiscovery.current = requestId;
      // Native Rust is the sole networking boundary. Browser fetch is never used.
      const response = await invoke<unknown>("discover_local_ai_models", { requestId, protocol, endpoint });
      const parsed = parseAiLocalInventory(response, protocol, endpoint);
      if (epoch.current.matches(request) && !offlineMode.getSnapshot()) {
        setInventory(parsed);
        if (savedModelHint && parsed.models.some((model) => model.id === savedModelHint)) {
          setSelectedModel(savedModelHint);
        } else if (savedModelHint) {
          setError("Saved model is no longer advertised by this server. Choose another model.");
        }
        if (!parsed.models.length) setError("The selected server has no available models.");
      }
    } catch {
      if (epoch.current.matches(request) && !offlineMode.getSnapshot()) {
        // Do not accidentally expose native error diagnostics or endpoint secrets.
        setError("Could not discover models. Check the endpoint, server availability, and protocol. HTTPS is not supported yet.");
      }
    } finally {
      if (activeDiscovery.current === requestId) activeDiscovery.current = null;
      if (epoch.current.matches(request)) setBusy(false);
    }
  }

  async function testSelectedModel() {
    if (!canTestAiLocalChoice(inventory, protocol, endpoint, selectedModel,
      offlineMode.getSnapshot(), saving || busy || testing)) return;
    // Ignore late results if the user edits the selection or enables offline mode.
    epoch.current.invalidate();
    const request = epoch.current.begin();
    setTesting(true);
    setTestResult(null);
    let requestId: number | null = null;
    try {
      // First register a native cancellation lease without making a network
      // request. A late registration is cancelled, never allowed to POST.
      requestId = await invoke<number>("begin_local_ai_model_test");
      if (!Number.isSafeInteger(requestId) || requestId <= 0) throw new Error("Invalid diagnostic lease");
      if (!epoch.current.matches(request) || offlineMode.getSnapshot()) {
        void invoke<void>("cancel_local_ai_model_test", { requestId }).catch(() => {});
        return;
      }
      activeProbe.current = requestId;
      // Native Rust rediscovers the model and sends only the fixed diagnostic.
      await invoke<void>("test_local_ai_model", { requestId, protocol, endpoint, model: selectedModel });
      if (epoch.current.matches(request) && !offlineMode.getSnapshot()) {
        setTestResult({ ok: true, message: "Local model responded to the fixed test prompt. This does not enable article chat." });
      }
    } catch {
      if (epoch.current.matches(request) && !offlineMode.getSnapshot()) {
        setTestResult({ ok: false, message: "Local inference test failed. Check that this model can generate responses on the selected server." });
      }
    } finally {
      if (activeProbe.current === requestId) activeProbe.current = null;
      if (epoch.current.matches(request)) setTesting(false);
    }
  }

  // Refresh only the saved list after a failed mutation. Never overwrite
  // unsaved endpoint/model edits with another window's values.
  async function refreshSavedProfiles(revision: number) {
    const request = profileReads.current.begin();
    try {
      const current = parseAiSavedLocalProfiles(await invoke<unknown>("read_local_ai_profiles"));
      if (profileReads.current.matches(request) && revision === editRevision.current) {
        setNeedsReview(true);
        setDeletePending(null);
        setSavedProfiles(current);
      }
    } catch {
      // Keep the user's draft if the native store is unavailable.
    }
  }

  async function activateProfile(name: string) {
    if (saving) return;
    const saved = savedProfiles.profiles.find((item) => item.name === name);
    if (!saved) return;
    invalidate();
    setProfileName(saved.name);
    setProtocol(saved.protocol);
    setEndpoint(saved.endpoint);
    setSavedModelHint(saved.model);
    const revision = editRevision.current;
    setSaving(true);
    try {
      const updated = parseAiSavedLocalProfiles(await invoke<unknown>("activate_local_ai_profile", { name, expectedProfiles: savedProfiles }));
      if (revision === editRevision.current) {
        setSavedProfiles(updated);
        setNeedsReview(false);
        setSavedNotice("Profile selected. Discover models to verify the server and model.");
      }
    } catch {
      if (revision === editRevision.current) {
        await refreshSavedProfiles(revision);
        setSavedNotice("Could not activate profile. Saved profiles were refreshed if available; review and retry.");
      }
    } finally {
      setSaving(false);
    }
  }

  async function disableActiveProfile() {
    if (saving || needsReview || savedProfiles.active === null) return;
    // Preserve the editor's draft, but do not retain a live model selection
    // after explicitly disabling the persisted active provider.
    invalidate(true);
    const revision = editRevision.current;
    setSaving(true);
    try {
      const updated = parseAiSavedLocalProfiles(await invoke<unknown>("deactivate_local_ai_profile", { expectedProfiles: savedProfiles }));
      if (revision === editRevision.current) {
        setSavedProfiles(updated);
        setNeedsReview(false);
        setSavedNotice("Active local AI profile disabled. Saved profiles remain available for explicit reactivation.");
      }
    } catch {
      if (revision === editRevision.current) {
        await refreshSavedProfiles(revision);
        setSavedNotice("Could not disable the active profile. Saved profiles were refreshed if available; review and retry.");
      }
    } finally {
      setSaving(false);
    }
  }

  function newProfile() {
    if (saving) return;
    invalidate();
    setProfileName("");
    setProtocol("ollama");
    setEndpoint("http://127.0.0.1:11434");
    setSavedNotice("Enter a profile name, discover models, and save your choice.");
  }

  async function removeProfile() {
    if (saving || !canConfirmAiLocalProfileDelete(deletePending, profileName, savedProfiles, needsReview)) return;
    const name = profileName;
    invalidate();
    const revision = editRevision.current;
    setSaving(true);
    try {
      const updated = parseAiSavedLocalProfiles(await invoke<unknown>("remove_local_ai_profile", { name, expectedProfiles: savedProfiles }));

      if (revision === editRevision.current) {
        setSavedProfiles(updated);
        setNeedsReview(false);
        const replacement = updated.profiles.find((item) => item.name === updated.active);
        setProfileName(replacement?.name ?? "");
        setProtocol(replacement?.protocol ?? "ollama");
        setEndpoint(replacement?.endpoint ?? "http://127.0.0.1:11434");
        setSavedModelHint(replacement?.model ?? "");
        setSavedNotice("Saved profile deleted. Any remaining model must be rediscovered.");
      }
    } catch {
      if (revision === editRevision.current) {
        await refreshSavedProfiles(revision);
        setSavedNotice("Could not delete profile. Saved profiles were refreshed if available; review and retry.");
      }
    } finally {
      setSaving(false);
    }
  }

  function discardDraftAndLoadSaved() {
    if (saving || !needsReview) return;
    // The saved list was independently parsed and revision-checked on read.
    // Clearing the draft also invalidates any in-flight discovery response.
    const latest = latestAiSavedLocalDraft(savedProfiles, profileName);
    invalidate();
    setProfileName(latest.name);
    setProtocol(latest.protocol);
    setEndpoint(latest.endpoint);
    setSavedModelHint(latest.modelHint);
    setNeedsReview(false);
    setSavedNotice(latest.name
      ? "Draft discarded. Current saved values loaded; discover models to verify availability."
      : "Draft discarded. No saved profiles remain; create a new profile to continue.");
  }

  async function saveChoice() {
    if (saving || testing || !canSaveAiLocalChoice(inventory, protocol, endpoint, selectedModel) || offlineMode.getSnapshot()) return;
    if (needsReview) {
      setSavedNotice("Review and acknowledge the newer saved profiles before saving your draft.");
      return;
    }
    if (!validAiLocalProfileName(profileName)) {
      setSavedNotice("Choose a profile name (1–40 ASCII letters, numbers, spaces, dots, hyphens or underscores).");
      return;
    }
    const requested = { name: profileName, protocol, endpoint, model: selectedModel };
    const revision = editRevision.current;
    profileReads.current.invalidate();
    setSaving(true);
    setSavedNotice(null);
    try {
      const updated = parseAiSavedLocalProfiles(await invoke<unknown>("upsert_local_ai_profile", { profile: requested, expectedProfiles: savedProfiles }));
      if (revision === editRevision.current && !offlineMode.getSnapshot()) {
        setSavedProfiles(updated);
        setNeedsReview(false);
        setSavedModelHint(selectedModel);
        setSavedNotice("Saved local profile. It will be revalidated on the next discovery.");
      }
    } catch {
      if (revision === editRevision.current) {
        await refreshSavedProfiles(revision);
        setSavedNotice("Could not save profile. Saved profiles were refreshed if available; review and retry.");
      }
    } finally {
      setSaving(false);
    }
  }

  const activeDestination = activeAiLocalProfileDestination(savedProfiles);
  const draftLocality = classifyAiLocalEndpoint(endpoint);

  return <section className="ai-local-settings" aria-label="Local AI provider selection">
    <div className="ai-local-inventory">
      <label htmlFor={`${id}-saved-profile`}>Saved local profiles</label>
      <select id={`${id}-saved-profile`} disabled={saving} value={savedProfiles.active ?? ""}
        onChange={(event) => { if (event.target.value) void activateProfile(event.target.value); }}>
        <option value="">Choose saved profile…</option>
        {savedProfiles.profiles.map((item) => <option key={item.name} value={item.name}>
          {item.name}{item.name === savedProfiles.active ? " · active" : ""}
        </option>)}
      </select>
      <button type="button" disabled={saving || needsReview || savedProfiles.active === null}
        onClick={() => void disableActiveProfile()}>Disable active local profile</button>
      <button type="button" disabled={saving} onClick={newProfile}>New profile</button>
      <button type="button" disabled={saving || needsReview || !savedProfiles.profiles.some((p) => p.name === profileName)}
        onClick={() => setDeletePending(profileName)}>Delete saved profile…</button>
      {deletePending !== null && <div role="alert" className="ai-local-inventory">
        <p>Delete saved profile <strong>{deletePending}</strong>? This removes its saved server and model selection, not the installed model.</p>
        <button type="button" disabled={saving || !canConfirmAiLocalProfileDelete(deletePending, profileName, savedProfiles, needsReview)}
          onClick={() => void removeProfile()}>Confirm delete profile</button>
        <button type="button" disabled={saving} onClick={() => setDeletePending(null)}>Cancel deletion</button>
      </div>}
      <label htmlFor={`${id}-profile-name`}>Profile name</label>
      <input id={`${id}-profile-name`} value={profileName} maxLength={40}
        disabled={saving} autoComplete="off" onChange={(event) => { invalidate(); setProfileName(event.target.value); }} />
      <p>Up to eight named local profiles. Disable leaves saved profiles intact but selects no active provider; selecting a saved profile reactivates it. Switching or restoring never contacts its server. Delete removes saved configuration, not installed models.</p>
    </div>
    <div className="ai-local-inventory" aria-label="Saved AI destination disclosure">
      {activeDestination ? <>
        <p>Active saved profile: <strong>{activeDestination.name}</strong> · {activeDestination.protocol === "ollama" ? "Ollama" : "OpenAI-compatible"} · <code>{activeDestination.endpoint}</code>.</p>
        <p>{aiLocalityDisclosure(activeDestination.locality)} This saved selection is not a connection or permission to send article passages.</p>
      </> : <p>No active local AI profile is saved.</p>}
      <p>Current editor destination: {draftLocality
        ? aiLocalityDisclosure(draftLocality)
        : "Not a permitted localhost/private-LAN HTTP endpoint."} Editing or restoring settings does not contact this destination.</p>
    </div>
    <p>Choose a server explicitly. No AI provider is contacted until you select <strong>Discover models</strong>. Public internet and DNS hostnames are rejected by the local provider adapter.</p>
    <form onSubmit={(event) => void discover(event)}>
      <label htmlFor={`${id}-protocol`}>Local server protocol</label>
      <select id={`${id}-protocol`} disabled={saving} value={protocol} onChange={(event) => {
        invalidate();
        setProtocol(event.target.value as AiLocalProtocol);
      }}>
        <option value="ollama">Ollama</option>
        <option value="openai-compatible">OpenAI-compatible (llama-server)</option>
      </select>
      <label htmlFor={`${id}-endpoint`}>Server URL</label>
      <input id={`${id}-endpoint`} type="url" required disabled={saving} value={endpoint} autoComplete="off" spellCheck={false}
        placeholder="http://127.0.0.1:11434" maxLength={512} onChange={(event) => {
          invalidate();
          setEndpoint(event.target.value);
        }} />
      <p className="ai-local-note">Use an explicit private-LAN IP address to connect to a model running on another machine. LAN traffic is network traffic, not offline inference. This preview supports HTTP only; do not send sensitive content over untrusted networks.</p>
      <button type="submit" disabled={saving || busy || testing || offline}>{busy ? "Discovering models…" : "Discover models"}</button>
    </form>
    {offline && <p role="status">Work offline is enabled. Localhost and LAN connections are disabled.</p>}
    {error && !offline && <p role="alert">{error}</p>}
    {savedNotice && <p role="status">{savedNotice}</p>}
    {needsReview && <div className="ai-local-inventory" role="alert">
      <p>Saved profiles changed while this editor was open. Your unsaved fields were preserved, but saving them could overwrite newer work.</p>
      <p>Current saved selection: <strong>{savedProfiles.active ?? "None"}</strong>. {(() => {
        const current = savedProfiles.profiles.find((item) => item.name === profileName);
        return current ? <>For <strong>{profileName}</strong>, the saved server is <code>{current.endpoint}</code> and model is <code>{current.model}</code>.</> : <>No profile named <strong>{profileName || "(unnamed)"}</strong> is currently saved.</>;
      })()}</p>
      <button type="button" disabled={saving} onClick={() => {
        setNeedsReview(false);
        setSavedNotice("Newer saved profiles acknowledged. Saving your current draft is now an explicit overwrite.");
      }}>I reviewed the saved state — allow saving my draft</button>
      <button type="button" disabled={saving} onClick={discardDraftAndLoadSaved}>
        Discard my draft and load latest saved values
      </button>
    </div>}
    {inventory && !offline && <div className="ai-local-inventory">
      <p role="status">Discovered {inventory.models.length} model(s) at <code>{inventory.endpoint}</code>.</p>
      <p>{aiLocalityDisclosure(inventory.locality)}</p>
      <label htmlFor={`${id}-model`}>Selected model</label>
      <select id={`${id}-model`} value={selectedModel} disabled={saving || !inventory.models.length}
        onChange={(event) => {
          cancelActiveProbe();
          epoch.current.invalidate();
          setTesting(false);
          setTestResult(null);
          setSelectedModel(event.target.value);
        }}>
        <option value="">Choose a discovered model…</option>
        {inventory.models.map((model) => <option key={model.id} value={model.id}>
          {model.id}{model.contextWindow ? ` · ${model.contextWindow.toLocaleString()} context tokens` : ""}
        </option>)}
      </select>
      {selectedModel && <>
        <p>Test selected model sends only the fixed text <strong>Reply OK.</strong> to this localhost/LAN server. No article text, bookmarks or credentials are sent. It does not save or enable a profile and may take up to two minutes. Switching offline or changing the selection cancels an active test, including a stalled model-inventory read; TCP connect and writes retain bounded timeouts.</p>
        <button type="button" disabled={!canTestAiLocalChoice(inventory, protocol, endpoint, selectedModel, offline, saving || busy || testing)}
          onClick={() => void testSelectedModel()}>{testing ? "Testing local model…" : "Test selected model"}</button>
      </>}
      {testResult && <p role={testResult.ok ? "status" : "alert"}>{testResult.message}</p>}
      {selectedModel && canSaveAiLocalChoice(inventory, protocol, endpoint, selectedModel) && <p role="status">Selected <strong>{selectedModel}</strong> on {inventory.locality === "localhost" ? "localhost" : "your LAN"}. Use Save local profile to retain this selection across restarts. Chat remains unavailable.</p>}
      {selectedModel && <button type="button" disabled={saving || testing || offline || needsReview || !canSaveAiLocalChoice(inventory, protocol, endpoint, selectedModel)} onClick={() => void saveChoice()}>{saving ? "Saving…" : "Save local profile"}</button>}
    </div>}
    <p>This page never selects a cloud provider or automatically falls back to one. Article-grounded AI answers, remote credentials and model inference are not wired to the reader yet.</p>
  </section>;
}

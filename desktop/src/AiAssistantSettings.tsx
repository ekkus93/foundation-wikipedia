import { useEffect, useId, useRef, useState, useSyncExternalStore, type FormEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { type AiLocalInventory, type AiLocalProtocol, aiLocalityDisclosure, parseAiLocalInventory, parseAiSavedLocalProfiles, validAiLocalProfileName, type AiSavedLocalProfiles } from "./aiLocalModels";
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
  const [savedModelHint, setSavedModelHint] = useState("");
  const [profileName, setProfileName] = useState("Default");
  const [savedProfiles, setSavedProfiles] = useState<AiSavedLocalProfiles>({ active: null, profiles: [] });
  const [saving, setSaving] = useState(false);
  const [savedNotice, setSavedNotice] = useState<string | null>(null);
  const editRevision = useRef(0);
  const [error, setError] = useState<string | null>(null);
  const epoch = useRef(new RequestEpoch());
  const offline = useSyncExternalStore(offlineMode.subscribe, offlineMode.getSnapshot, offlineMode.getSnapshot);

  useEffect(() => () => { epoch.current.invalidate(); }, []);
  useEffect(() => {
    let active = true;
    const revision = editRevision.current;
    void invoke<unknown>("read_local_ai_profiles").then(parseAiSavedLocalProfiles).then((saved) => {
      if (!active || revision !== editRevision.current) return;
      setSavedProfiles(saved);
      const profile = saved.profiles.find((item) => item.name === saved.active);
      if (!profile) return;
      setProtocol(profile.protocol);
      setEndpoint(profile.endpoint);
      setSavedModelHint(profile.model);
      setProfileName(profile.name);
      setSavedNotice("Saved local profile loaded. Discover models again to verify availability.");
    }).catch(() => {
      if (active && revision === editRevision.current) setSavedNotice("Could not restore the saved local profile.");
    });
    return () => { active = false; };
  }, []);
  // Network permission may be withdrawn during an in-flight discovery.
  useEffect(() => offlineMode.subscribe(() => {
    if (offlineMode.getSnapshot()) {
      epoch.current.invalidate();
      setBusy(false);
      setInventory(null);
      setSelectedModel("");
      setError(null);
    }
  }), [offlineMode]);

  function invalidate(preserveSavedHint = false) {
    epoch.current.invalidate();
    setBusy(false);
    setError(null);
    setInventory(null);
    setSelectedModel("");
    if (!preserveSavedHint) setSavedModelHint("");
    setSavedNotice(null);
    editRevision.current += 1;
  }

  async function discover(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (busy || offlineMode.getSnapshot()) return;
    invalidate(true);
    if (!endpoint || endpoint.length > 512 || endpoint.trim() !== endpoint) {
      setError("Enter an exact localhost or private-IP URL without spaces.");
      return;
    }
    const request = epoch.current.begin();
    setBusy(true);
    try {
      // Native Rust is the sole networking boundary. Browser fetch is never used.
      const response = await invoke<unknown>("discover_local_ai_models", { protocol, endpoint });
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
      if (epoch.current.matches(request)) setBusy(false);
    }
  }

  // Refresh only the saved list after a failed mutation. Never overwrite
  // unsaved endpoint/model edits with another window's values.
  async function refreshSavedProfiles(revision: number) {
    try {
      const current = parseAiSavedLocalProfiles(await invoke<unknown>("read_local_ai_profiles"));
      if (revision === editRevision.current) setSavedProfiles(current);
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

  function newProfile() {
    if (saving) return;
    invalidate();
    setProfileName("");
    setProtocol("ollama");
    setEndpoint("http://127.0.0.1:11434");
    setSavedNotice("Enter a profile name, discover models, and save your choice.");
  }

  async function removeProfile() {
    if (saving || !savedProfiles.profiles.some((item) => item.name === profileName)) return;
    const name = profileName;
    invalidate();
    const revision = editRevision.current;
    setSaving(true);
    try {
      const updated = parseAiSavedLocalProfiles(await invoke<unknown>("remove_local_ai_profile", { name, expectedProfiles: savedProfiles }));
      if (revision === editRevision.current) {
        setSavedProfiles(updated);
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

  async function saveChoice() {
    if (saving || !inventory || !selectedModel || offlineMode.getSnapshot()) return;
    if (!validAiLocalProfileName(profileName)) {
      setSavedNotice("Choose a profile name (1–40 ASCII letters, numbers, spaces, dots, hyphens or underscores).");
      return;
    }
    const requested = { name: profileName, protocol, endpoint, model: selectedModel };
    const revision = editRevision.current;
    setSaving(true);
    setSavedNotice(null);
    try {
      const updated = parseAiSavedLocalProfiles(await invoke<unknown>("upsert_local_ai_profile", { profile: requested, expectedProfiles: savedProfiles }));
      if (revision === editRevision.current && !offlineMode.getSnapshot()) {
        setSavedProfiles(updated);
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

  return <section className="ai-local-settings" aria-label="Local AI provider selection">
    <div className="ai-local-inventory">
      <label htmlFor={`${id}-saved-profile`}>Saved local profiles</label>
      <select id={`${id}-saved-profile`} disabled={saving} value={savedProfiles.profiles.some((p) => p.name === profileName) ? profileName : ""}
        onChange={(event) => { if (event.target.value) void activateProfile(event.target.value); }}>
        <option value="">Choose saved profile…</option>
        {savedProfiles.profiles.map((item) => <option key={item.name} value={item.name}>
          {item.name}{item.name === savedProfiles.active ? " · active" : ""}
        </option>)}
      </select>
      <button type="button" disabled={saving} onClick={newProfile}>New profile</button>
      <button type="button" disabled={saving || !savedProfiles.profiles.some((p) => p.name === profileName)}
        onClick={() => void removeProfile()}>Delete saved profile</button>
      <label htmlFor={`${id}-profile-name`}>Profile name</label>
      <input id={`${id}-profile-name`} value={profileName} maxLength={40}
        disabled={saving} autoComplete="off" onChange={(event) => { invalidate(); setProfileName(event.target.value); }} />
      <p>Up to eight named local profiles. Switching or restoring a profile never contacts its server. Delete removes saved configuration, not installed models.</p>
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
      <button type="submit" disabled={saving || busy || offline}>{busy ? "Discovering models…" : "Discover models"}</button>
    </form>
    {offline && <p role="status">Work offline is enabled. Localhost and LAN connections are disabled.</p>}
    {error && !offline && <p role="alert">{error}</p>}
    {savedNotice && <p role="status">{savedNotice}</p>}
    {inventory && !offline && <div className="ai-local-inventory">
      <p role="status">Discovered {inventory.models.length} model(s) at <code>{inventory.endpoint}</code>.</p>
      <p>{aiLocalityDisclosure(inventory.locality)}</p>
      <label htmlFor={`${id}-model`}>Selected model</label>
      <select id={`${id}-model`} value={selectedModel} disabled={saving || !inventory.models.length}
        onChange={(event) => setSelectedModel(event.target.value)}>
        <option value="">Choose a discovered model…</option>
        {inventory.models.map((model) => <option key={model.id} value={model.id}>
          {model.id}{model.contextWindow ? ` · ${model.contextWindow.toLocaleString()} context tokens` : ""}
        </option>)}
      </select>
      {selectedModel && <p role="status">Selected <strong>{selectedModel}</strong> on {inventory.locality === "localhost" ? "localhost" : "your LAN"}. Use Save local profile to retain this selection across restarts. Chat remains unavailable.</p>}
      {selectedModel && <button type="button" disabled={saving || offline} onClick={() => void saveChoice()}>{saving ? "Saving…" : "Save local profile"}</button>}
    </div>}
    <p>This page never selects a cloud provider or automatically falls back to one. Article-grounded AI answers, remote credentials and model inference are not wired to the reader yet.</p>
  </section>;
}

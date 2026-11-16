import { useEffect, useState, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { HOSTED_LABELS, HOSTED_PROVIDERS, loadHostedModels, saveHostedModel, validModelName, type HostedProviderId } from "./hostedModels";
import type { ReaderOfflineMode } from "./readerOfflineMode";

/**
 * Settings › AI Assistant › Hosted models: an API key per provider, kept in
 * the OS secret store (it is sent to the native side once and never shown
 * again), the model name to use, and an explicit connection test. Using a
 * hosted model sends questions and article passages over the internet, and
 * every chat request is shown for review first.
 */
export default function HostedAiSettings({ offlineMode }: { offlineMode: ReaderOfflineMode }) {
  const offline = useSyncExternalStore(offlineMode.subscribe, offlineMode.getSnapshot, offlineMode.getSnapshot);
  const [configured, setConfigured] = useState<Partial<Record<HostedProviderId, boolean>>>({});
  const [models, setModels] = useState(loadHostedModels);
  const [keyDrafts, setKeyDrafts] = useState<Partial<Record<HostedProviderId, string>>>({});
  const [modelDrafts, setModelDrafts] = useState<Partial<Record<HostedProviderId, string>>>(() => ({ ...loadHostedModels() }));
  const [confirmDelete, setConfirmDelete] = useState<HostedProviderId | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  async function refresh() {
    const next: Partial<Record<HostedProviderId, boolean>> = {};
    for (const provider of HOSTED_PROVIDERS) {
      try {
        next[provider] = await invoke<boolean>("hosted_ai_key_configured", { provider }) === true;
      } catch {
        next[provider] = false;
      }
    }
    setConfigured(next);
  }

  useEffect(() => {
    void refresh();
  }, []);

  async function run(action: () => Promise<string>) {
    setBusy(true);
    setMessage(null);
    try {
      setMessage(await action());
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="hosted-ai" aria-label="Hosted models">
      <h2>Hosted models (internet)</h2>
      <p>
        Optional. With an API key, chat can ask Anthropic or OpenAI. Questions and the article
        passages shown in each request review are then sent over the internet to that company.
        Keys are kept in your system's secure credential storage and are never shown again.
      </p>
      {HOSTED_PROVIDERS.map((provider) => {
        const label = HOSTED_LABELS[provider];
        const modelDraft = modelDrafts[provider] ?? "";
        return (
          <fieldset key={provider} className="hosted-provider">
            <legend>{label}</legend>
            <p>{configured[provider] ? "An API key is saved." : "No API key is saved."}</p>
            <label>
              {configured[provider] ? "Replace API key" : "API key"}
              <input type="password" autoComplete="off" spellCheck={false} value={keyDrafts[provider] ?? ""}
                onChange={(event) => setKeyDrafts((drafts) => ({ ...drafts, [provider]: event.target.value }))} />
            </label>
            <button type="button" disabled={busy || !(keyDrafts[provider] ?? "").trim()} onClick={() => void run(async () => {
              await invoke("save_hosted_ai_key", { provider, key: keyDrafts[provider] ?? "" });
              setKeyDrafts((drafts) => ({ ...drafts, [provider]: "" }));
              await refresh();
              return `${label} API key saved in secure storage.`;
            })}>Save key</button>
            {configured[provider] && (confirmDelete === provider ? (
              <span role="group" aria-label={`Confirm deleting the ${label} key`}>
                Delete the saved {label} key?
                <button type="button" disabled={busy} onClick={() => void run(async () => {
                  setConfirmDelete(null);
                  await invoke("delete_hosted_ai_key", { provider });
                  await refresh();
                  return `${label} API key deleted.`;
                })}>Delete</button>
                <button type="button" onClick={() => setConfirmDelete(null)}>Cancel</button>
              </span>
            ) : <button type="button" disabled={busy} onClick={() => setConfirmDelete(provider)}>Delete key…</button>)}
            <label>
              Model
              <input value={modelDraft} maxLength={128} spellCheck={false}
                placeholder={provider === "anthropic" ? "for example claude-sonnet-4-5" : "for example gpt-5"}
                onChange={(event) => setModelDrafts((drafts) => ({ ...drafts, [provider]: event.target.value.trim() }))} />
            </label>
            <button type="button" disabled={busy || !validModelName(modelDraft) || models[provider] === modelDraft}
              onClick={() => { setModels(saveHostedModel(provider, modelDraft)); setMessage(`${label} chat will use ${modelDraft}.`); }}>
              Save model
            </button>
            <button type="button" disabled={busy || offline || !configured[provider] || !validModelName(models[provider])}
              onClick={() => void run(async () => {
                const result = await invoke<{ reply?: unknown }>("check_hosted_ai_connection", { provider, model: models[provider] });
                const reply = typeof result?.reply === "string" ? result.reply.slice(0, 80) : "";
                return `${label} answered: “${reply}”`;
              })}>
              Test connection (sends a short test message)
            </button>
          </fieldset>
        );
      })}
      {offline && <p>Work offline is on, so the connection test is unavailable.</p>}
      {message && <p role="status">{message}</p>}
    </section>
  );
}

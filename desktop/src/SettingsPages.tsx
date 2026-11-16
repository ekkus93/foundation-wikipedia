import { useEffect, useState, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { EDITIONS, isEdition, loadDefaultEdition, saveDefaultEdition } from "./defaultEdition";
import type { ReaderOfflineMode } from "./readerOfflineMode";

/** STATE-002 settings pages that previously had no content. */

function formatBytes(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${bytes} bytes`;
}

export function GeneralSettings() {
  const [edition, setEdition] = useState(loadDefaultEdition);
  const [message, setMessage] = useState<string | null>(null);
  return <section aria-label="General settings">
    <label>Default Wikipedia edition for new tabs{" "}
      <select value={edition} onChange={(event) => {
        if (!isEdition(event.target.value)) return;
        setEdition(event.target.value);
        setMessage(saveDefaultEdition(event.target.value)
          ? "Saved. New tabs start with this edition."
          : "This choice could not be saved; it applies until the app closes.");
      }}>
        {EDITIONS.map(([id, label]) => <option key={id} value={id}>{label}</option>)}
      </select>
    </label>
    {message && <p role="status">{message}</p>}
    <p>Each tab can still switch edition in the reader. Installed packs keep their own edition.</p>
  </section>;
}

interface StorageUsage {
  packs: [string, number][];
  downloads: number;
  staging: number;
  personal_data: number;
}

function parseUsage(value: unknown): StorageUsage {
  const item = value as Record<string, unknown>;
  const count = (n: unknown) => (typeof n === "number" && Number.isSafeInteger(n) && n >= 0 ? n : 0);
  const packs = Array.isArray(item?.packs) ? item.packs.slice(0, 1000)
    .filter((pack): pack is [string, number] => Array.isArray(pack) && typeof pack[0] === "string")
    .map(([id, bytes]) => [id.slice(0, 256), count(bytes)] as [string, number]) : [];
  return { packs, downloads: count(item?.downloads), staging: count(item?.staging), personal_data: count(item?.personal_data) };
}

export function StorageSettings({ onOpenPacks }: { onOpenPacks: () => void }) {
  const [usage, setUsage] = useState<StorageUsage | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    void invoke<unknown>("offline_storage_usage")
      .then((value) => setUsage(parseUsage(value)))
      .catch(() => setError("Storage use could not be measured."));
  }, []);
  const packTotal = usage?.packs.reduce((sum, [, bytes]) => sum + bytes, 0) ?? 0;
  return <section aria-label="Storage">
    {error && <p role="alert">{error}</p>}
    {!usage && !error && <p role="status">Measuring…</p>}
    {usage && <>
      <h2>Offline packs: {formatBytes(packTotal)}</h2>
      {usage.packs.length === 0 ? <p>No offline packs are installed.</p>
        : <ul>{usage.packs.map(([id, bytes]) => <li key={id}>{id}: {formatBytes(bytes)}</li>)}</ul>}
      <button type="button" onClick={onOpenPacks}>Manage packs…</button>
      <h2>Other data</h2>
      <ul>
        <li>Partial pack downloads (kept to resume): {formatBytes(usage.downloads)}</li>
        <li>Interrupted installs (cleared automatically): {formatBytes(usage.staging)}</li>
        <li>Personal reading data (bookmarks, history, chats, settings): {formatBytes(usage.personal_data)}</li>
      </ul>
      <p>Online articles are not stored; their images are cached only by the system web view.</p>
    </>}
  </section>;
}

export function PrivacySettings({ offlineMode, onOpenHistory }: { offlineMode: ReaderOfflineMode; onOpenHistory: () => void }) {
  const offline = useSyncExternalStore(offlineMode.subscribe, offlineMode.getSnapshot, offlineMode.getSnapshot);
  return <section aria-label="Privacy and network">
    <label><input type="checkbox" checked={offline} onChange={(event) => offlineMode.setOffline(event.target.checked)} />
      {" "}Work offline (no new network requests from the reader)</label>
    <h2>What goes over the network</h2>
    <ul>
      <li>Online reading: searches and article requests go to Wikipedia (wikipedia.org); images and formulas load from Wikimedia servers. Nothing is requested while Work offline is on.</li>
      <li>Links to other sites open in your browser only after you confirm each one.</li>
      <li>AI chat: nothing is sent until you review a request and choose Send. A local or local-network model receives the question and the listed passages; a hosted provider (Anthropic or OpenAI) receives them over the internet.</li>
      <li>Installed packs, search in packs and saved data stay on this computer. The app sends no analytics or usage data.</li>
    </ul>
    <h2>Saved data</h2>
    <p>Bookmarks, visit history (off by default) and chat answers (saved only with history on) stay in a private folder on this computer. API keys stay in your system's secure credential storage.</p>
    <button type="button" onClick={onOpenHistory}>History & Saved Content…</button>
  </section>;
}

export function AdvancedSettings() {
  const [location, setLocation] = useState<string | null>(null);
  const [check, setCheck] = useState<string | null>(null);
  useEffect(() => {
    void invoke<string>("app_data_location").then((path) => setLocation(typeof path === "string" ? path : null)).catch(() => {});
  }, []);
  return <section aria-label="Advanced">
    <h2>Data folder</h2>
    <p>{location ?? "Unknown"}</p>
    <p>Offline packs are in its <code>offline-library</code> folder and personal reading data in <code>reader-state</code>. Use the app's own controls to change them.</p>
    <h2>Check installed packs</h2>
    <button type="button" onClick={() => void invoke<unknown>("list_offline_packs").then((packs) => {
      const list = Array.isArray(packs) ? packs as Array<Record<string, unknown>> : [];
      const damaged = list.filter((pack) => pack?.damaged === true).length;
      setCheck(list.length === 0 ? "No packs are installed."
        : damaged === 0 ? `${list.length} pack${list.length === 1 ? "" : "s"} listed; none is damaged. Each pack is fully verified again when it is opened.`
          : `${damaged} of ${list.length} packs are damaged; remove and reinstall them in Wikipedia & Offline Data.`);
    }).catch(() => setCheck("Installed packs could not be checked."))}>Check</button>
    {check && <p role="status">{check}</p>}
  </section>;
}

export function AboutSettings() {
  const [version, setVersion] = useState<string | null>(null);
  useEffect(() => {
    void getVersion().then(setVersion).catch(() => {});
  }, []);
  return <section aria-label="About">
    <p>Foundation Wikipedia reader {version ? `version ${version}` : ""} — development preview.</p>
    <p>An independent reader for Wikipedia; it is not made by or affiliated with the Wikimedia Foundation.
      Wikipedia is a trademark of the Wikimedia Foundation.</p>
    <p>Article text is available under the Creative Commons Attribution-ShareAlike licence (CC BY-SA 4.0)
      from its authors; each article shows the exact revision it came from. Images have their own licences, shown on their Wikimedia pages.</p>
  </section>;
}

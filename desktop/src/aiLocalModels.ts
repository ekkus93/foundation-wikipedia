/** Untrusted native inventory response; this view never stores credentials. */
export type AiLocalProtocol = "ollama" | "openai-compatible";
export type AiLocality = "localhost" | "local-network";
export interface AiLocalModel { id: string; contextWindow: number | null }
export interface AiLocalInventory {
  protocol: AiLocalProtocol;
  endpoint: string;
  locality: AiLocality;
  models: AiLocalModel[];
}

// Keep the UI's model-label safety rules aligned with wiki_ai::valid_model_id.
// The Rust adapter independently validates every model before publishing it.
export function validAiLocalModelId(id: unknown): id is string {
  if (typeof id !== "string") return false;
  // Rust strings cannot contain lone UTF-16 surrogate halves. Fail closed at
  // the JS IPC boundary instead of displaying a substituted replacement glyph.
  for (let i = 0; i < id.length; i += 1) {
    const code = id.charCodeAt(i);
    if (code >= 0xd800 && code <= 0xdbff) {
      const next = id.charCodeAt(++i);
      if (!(next >= 0xdc00 && next <= 0xdfff)) return false;
    } else if (code >= 0xdc00 && code <= 0xdfff) return false;
  }
  return id.length > 0 && id.trim() === id &&
    new TextEncoder().encode(id).length <= 256 &&
    !/[\u0000-\u001f\u007f-\u009f\u00ad\u034f\u061c\u180e\u200b-\u200f\u2028-\u202e\u2060-\u206f\ufe00-\ufe0f\ufeff\u{e0100}-\u{e01ef}]/u.test(id);
}

/** Conservative UI-side subset of Rust LocalEndpoint::parse; never grants network access. */
export function classifyAiLocalEndpoint(value: unknown): AiLocality | null {
  if (typeof value !== "string" || value.length === 0 || value.length > 512 ||
      /[^\x21-\x7e]|[@?#\\%]/u.test(value)) return null;
  const match = /^http:\/\/([^/]+)(?:\/(?:v1\/?)?)?$/.exec(value);
  if (!match) return null;
  const authority = match[1];
  const bracketed = authority.startsWith("[");
  const parts = bracketed
    ? /^(\[[0-9a-fA-F:]+\])(?::([0-9]+))?$/.exec(authority)
    : /^([A-Za-z0-9.]+)(?::([0-9]+))?$/.exec(authority);
  if (!parts) return null;
  if (parts[2] !== undefined) {
    const port = Number(parts[2]);
    if (!Number.isInteger(port) || port < 1 || port > 65535) return null;
  }
  const host = parts[1];
  if (bracketed) {
    try {
      // URL validates IPv6 syntax; inspect the canonical address, not a label.
      const canonical = new URL(value).hostname;
      if (canonical === "[::1]") return "localhost";
      const first = Number.parseInt(canonical.slice(1).split(":")[0], 16);
      return first >= 0xfc00 && first <= 0xfdff ? "local-network" : null;
    } catch {
      return null;
    }
  }
  if (host.toLowerCase() === "localhost") return "localhost";
  const octets = host.split(".");
  if (octets.length !== 4 || !octets.every((part) =>
    /^[0-9]{1,3}$/.test(part) && (part === "0" || !part.startsWith("0")) &&
    Number(part) <= 255)) return null;
  const [a, b] = octets.map(Number);
  if (a === 127) return "localhost";
  return a === 10 || (a === 172 && b >= 16 && b <= 31) ||
    (a === 192 && b === 168) ? "local-network" : null;
}

/** Fail closed if the native response does not describe the requested host. */
export function parseAiLocalInventory(value: unknown, protocol: AiLocalProtocol, endpoint: string): AiLocalInventory {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("Invalid local model inventory");
  }
  const result = value as Record<string, unknown>;
  if (result.protocol !== protocol || result.endpoint !== endpoint ||
      classifyAiLocalEndpoint(endpoint) === null ||
      result.locality !== classifyAiLocalEndpoint(endpoint) ||
      !Array.isArray(result.models) || result.models.length > 256) {
    throw new Error("Invalid local model inventory");
  }
  const seen = new Set<string>();
  const models: AiLocalModel[] = [];
  for (const row of result.models) {
    if (typeof row !== "object" || row === null || Array.isArray(row)) {
      throw new Error("Invalid local model inventory");
    }
    const model = row as Record<string, unknown>;
    const context = model.contextWindow;
    if (!validAiLocalModelId(model.id) || seen.has(model.id) ||
        (context !== null && (!Number.isInteger(context) || !Number.isSafeInteger(context) ||
        (context as number) <= 0 || (context as number) > 0xffffffff))) {
      throw new Error("Invalid local model inventory");
    }
    seen.add(model.id);
    models.push({ id: model.id, contextWindow: context as number | null });
  }
  return {
    protocol,
    endpoint,
    locality: result.locality as AiLocality,
    models,
  };
}

/** A selected model can be saved only from this exact, currently verified inventory.
 * Editing the endpoint/protocol or supplying an unadvertised ID must not
 * turn a stale discovery result into a durable profile hint.
 */
export function canSaveAiLocalChoice(
  inventory: AiLocalInventory | null,
  protocol: AiLocalProtocol,
  endpoint: string,
  selectedModel: string,
): boolean {
  return inventory !== null &&
    (protocol === "ollama" || protocol === "openai-compatible") &&
    inventory.protocol === protocol && inventory.endpoint === endpoint &&
    inventory.locality === classifyAiLocalEndpoint(endpoint) &&
    validAiLocalModelId(selectedModel) &&
    inventory.models.some((model) => model.id === selectedModel);
}

export function aiLocalityDisclosure(locality: AiLocality): string {
  return locality === "localhost"
    ? "Localhost server — connected over TCP/IP; not an embedded on-device model."
    : "Local network server — sends selected conversation text over your LAN; unavailable offline.";
}
/** A saved profile is a hint, not permission to connect or invoke inference. */
export interface AiSavedLocalProfile {
  protocol: AiLocalProtocol;
  endpoint: string;
  model: string;
}

export function parseAiSavedLocalProfile(input: unknown): AiSavedLocalProfile | null {
  if (input === null) return null;
  if (typeof input !== "object" || Array.isArray(input)) throw new Error("Invalid saved AI profile");
  const row = input as Record<string, unknown>;
  if (Object.keys(row).sort().join(",") !== "endpoint,model,protocol" ||
      !(row.protocol === "ollama" || row.protocol === "openai-compatible") ||
      typeof row.endpoint !== "string" || classifyAiLocalEndpoint(row.endpoint) === null ||
      !validAiLocalModelId(row.model)) {
    throw new Error("Invalid saved AI profile");
  }
  return { protocol: row.protocol, endpoint: row.endpoint, model: row.model };
}
/** Read-only saved profiles; no saved entry authorizes an outbound request. */
export interface AiNamedLocalProfile extends AiSavedLocalProfile { name: string }
export interface AiSavedLocalProfiles {
  /** Monotonic persisted edit generation; prevents identical-value ABA replay. */
  revision: number;
  active: string | null;
  profiles: AiNamedLocalProfile[];
}

/** Display the *persisted active* destination separately from any unsaved editor
 * draft. Reading this metadata never grants permission to connect or infer.
 */
export function activeAiLocalProfileDestination(saved: AiSavedLocalProfiles): {
  name: string;
  protocol: AiLocalProtocol;
  endpoint: string;
  locality: AiLocality;
} | null {
  const active = saved.profiles.find((profile) => profile.name === saved.active);
  if (!active) return null;
  const locality = classifyAiLocalEndpoint(active.endpoint);
  if (locality === null) return null;
  return { name: active.name, protocol: active.protocol, endpoint: active.endpoint, locality };
}

/** Compare native profile snapshots without interpreting a saved hint as consent. */
export function sameAiSavedLocalProfiles(a: AiSavedLocalProfiles, b: AiSavedLocalProfiles): boolean {
  return a.revision === b.revision && a.active === b.active && a.profiles.length === b.profiles.length &&
    a.profiles.every((profile, index) => {
      const other = b.profiles[index];
      return profile.name === other.name && profile.protocol === other.protocol &&
        profile.endpoint === other.endpoint && profile.model === other.model;
    });
}

export function validAiLocalProfileName(name: unknown): name is string {
  return typeof name === "string" && name.length <= 40 && name.trim() === name &&
    /^[A-Za-z0-9][A-Za-z0-9 ._-]*$/.test(name);
}

export function parseAiSavedLocalProfiles(input: unknown): AiSavedLocalProfiles {
  if (typeof input !== "object" || input === null || Array.isArray(input)) {
    throw new Error("Invalid saved AI profiles");
  }
  const raw = input as Record<string, unknown>;
  const keys = Object.keys(raw).sort().join(",");
  // Pre-revision v2 records remain readable as generation zero.
  const revision = keys === "active,profiles" ? 0 : raw.revision;
  if (!(keys === "active,profiles" || keys === "active,profiles,revision") ||
      !Number.isSafeInteger(revision) || (revision as number) < 0 ||
      !Array.isArray(raw.profiles) || raw.profiles.length > 8) {
    throw new Error("Invalid saved AI profiles");
  }
  const profiles: AiNamedLocalProfile[] = [];
  const names = new Set<string>();
  for (const item of raw.profiles) {
    if (typeof item !== "object" || item === null || Array.isArray(item)) {
      throw new Error("Invalid saved AI profiles");
    }
    const candidate = item as Record<string, unknown>;
    const { name, protocol, endpoint, model } = candidate;
    if (Object.keys(candidate).sort().join(",") !== "endpoint,model,name,protocol" ||
        !validAiLocalProfileName(name) || names.has(name.toLowerCase())) {
      throw new Error("Invalid saved AI profiles");
    }
    names.add(name.toLowerCase());
    try {
      parseAiSavedLocalProfile({ protocol, endpoint, model });
    } catch {
      throw new Error("Invalid saved AI profiles");
    }
    profiles.push({ name, protocol: protocol as AiLocalProtocol, endpoint: endpoint as string, model: model as string });
  }
  if (raw.active !== null &&
      (typeof raw.active !== "string" || !profiles.some((p) => p.name === raw.active))) {
    throw new Error("Invalid saved AI profiles");
  }
  if (new TextEncoder().encode(JSON.stringify(input)).length > 4096) {
    throw new Error("Invalid saved AI profiles");
  }
  return { revision: revision as number, active: raw.active as string | null, profiles };
}

/** Sticky review fence: refreshing a CAS snapshot must not approve a stale draft. */
export function needsAiProfileReview(previous: AiSavedLocalProfiles, current: AiSavedLocalProfiles, pending = false): boolean {
  return pending || !sameAiSavedLocalProfiles(previous, current);
}

/** Replace an unsaved conflict draft with a fresh, validated SQLite snapshot.
 * Prefer the draft's named profile if it still exists; otherwise load the
 * current active profile. Never interpret the stored model as discovered.
 */
export function latestAiSavedLocalDraft(saved: AiSavedLocalProfiles, preferredName: string): {
  name: string;
  protocol: AiLocalProtocol;
  endpoint: string;
  modelHint: string;
} {
  const profile = saved.profiles.find((item) => item.name === preferredName) ??
    saved.profiles.find((item) => item.name === saved.active);
  return profile
    ? { name: profile.name, protocol: profile.protocol, endpoint: profile.endpoint, modelHint: profile.model }
    : { name: "", protocol: "ollama", endpoint: "http://127.0.0.1:11434", modelHint: "" };
}

/** Deleting a saved profile requires an explicit, still-current selection. */
export function canConfirmAiLocalProfileDelete(
  pendingName: string | null,
  selectedName: string,
  saved: AiSavedLocalProfiles,
  needsReview: boolean,
): boolean {
  return pendingName !== null && pendingName === selectedName && !needsReview &&
    saved.profiles.some((profile) => profile.name === selectedName);
}

/** A visible but unfocused window must eventually observe other processes. */
export function shouldPollAiProfiles(visibilityState: string): boolean {
  return visibilityState === "visible";
}

/** Coalesce concurrent native refresh triggers; never starve a slow read. */
export class AiProfileRefreshGate {
  private inFlight = false;
  private queued = false;

  begin(): boolean {
    if (this.inFlight) {
      this.queued = true;
      return false;
    }
    this.inFlight = true;
    return true;
  }

  finish(): boolean {
    this.inFlight = false;
    const queued = this.queued;
    this.queued = false;
    return queued;
  }
}

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

/** Fail closed if the native response does not describe the requested host. */
export function parseAiLocalInventory(value: unknown, protocol: AiLocalProtocol, endpoint: string): AiLocalInventory {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("Invalid local model inventory");
  }
  const result = value as Record<string, unknown>;
  if (result.protocol !== protocol || result.endpoint !== endpoint ||
      !(result.locality === "localhost" || result.locality === "local-network") ||
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
      typeof row.endpoint !== "string" || !row.endpoint.startsWith("http://") ||
      row.endpoint.trim() !== row.endpoint || row.endpoint.length > 512 ||
      /[\s\u0000-\u001f\u007f-\u009f@?#\\]/u.test(row.endpoint) ||
      !validAiLocalModelId(row.model)) {
    throw new Error("Invalid saved AI profile");
  }
  return { protocol: row.protocol, endpoint: row.endpoint, model: row.model };
}
/** Read-only saved profiles; no saved entry authorizes an outbound request. */
export interface AiNamedLocalProfile extends AiSavedLocalProfile { name: string }
export interface AiSavedLocalProfiles {
  active: string | null;
  profiles: AiNamedLocalProfile[];
}

/** Compare native profile snapshots without interpreting a saved hint as consent. */
export function sameAiSavedLocalProfiles(a: AiSavedLocalProfiles, b: AiSavedLocalProfiles): boolean {
  return a.active === b.active && a.profiles.length === b.profiles.length &&
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
  if (Object.keys(raw).sort().join(",") !== "active,profiles" ||
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
  if (profiles.length === 0 ? raw.active !== null :
      typeof raw.active !== "string" || !profiles.some((p) => p.name === raw.active)) {
    throw new Error("Invalid saved AI profiles");
  }
  if (new TextEncoder().encode(JSON.stringify(input)).length > 4096) {
    throw new Error("Invalid saved AI profiles");
  }
  return { active: raw.active as string | null, profiles };
}

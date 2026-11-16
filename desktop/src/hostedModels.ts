/** Hosted AI providers the desktop chat can use (keys live in OS secret
 * storage; only these non-secret model names are kept in the web UI). */
export const HOSTED_PROVIDERS = ["anthropic", "openai"] as const;
export type HostedProviderId = (typeof HOSTED_PROVIDERS)[number];

export const HOSTED_LABELS: Record<HostedProviderId, string> = { anthropic: "Anthropic", openai: "OpenAI" };

const STORAGE_KEY = "hostedAiModels.v1";

/** A plausible model ID: printable ASCII, no spaces, at most 128 bytes. */
export function validModelName(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= 128 && /^[\x21-\x7e]+$/u.test(value);
}

export type HostedModels = Partial<Record<HostedProviderId, string>>;

/** Read saved model names; anything malformed is ignored. */
export function parseHostedModels(raw: string | null): HostedModels {
  if (!raw) return {};
  try {
    const value: unknown = JSON.parse(raw);
    if (typeof value !== "object" || value === null || Array.isArray(value)) return {};
    const models: HostedModels = {};
    for (const provider of HOSTED_PROVIDERS) {
      const model = (value as Record<string, unknown>)[provider];
      if (validModelName(model)) models[provider] = model;
    }
    return models;
  } catch {
    return {};
  }
}

export function loadHostedModels(): HostedModels {
  try {
    return parseHostedModels(localStorage.getItem(STORAGE_KEY));
  } catch {
    return {};
  }
}

export function saveHostedModel(provider: HostedProviderId, model: string | null): HostedModels {
  const next = { ...loadHostedModels() };
  if (model === null) delete next[provider];
  else if (validModelName(model)) next[provider] = model;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
  } catch {
    // Model names are a convenience; the chat asks again if they are lost.
  }
  return next;
}

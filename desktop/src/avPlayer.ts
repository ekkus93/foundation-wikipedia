/** MOBPACK-003: the pure parts of the in-app audio/video player. */

/** Shown once, before the app first contacts Wikimedia's media servers. */
export const AV_STREAM_DISCLOSURE =
  "Audio and video are not stored in packs. To play this, the app connects to Wikimedia's " +
  "media servers (upload.wikimedia.org). Wikimedia will see your IP address and which " +
  "file you play, as it would in a browser. Nothing else is sent, and nothing is saved " +
  "on this device. This notice is shown once.";

/** A candidate that has not started playing within this long gives way to the next. */
export const AV_START_TIMEOUT_MS = 30_000;

export const AV_DISCLOSURE_KEY = "foundation-wikipedia.avStreamDisclosure";
const ACCEPTED = "v1:accepted";

export function encodeDisclosure(): string {
  return ACCEPTED;
}

export function disclosureAccepted(value: string | null | undefined): boolean {
  return value === ACCEPTED;
}

/** Whether the notice was accepted before; storage problems mean "not yet". */
export function loadDisclosureAccepted(): boolean {
  try {
    return disclosureAccepted(localStorage.getItem(AV_DISCLOSURE_KEY));
  } catch {
    return false;
  }
}

export function saveDisclosureAccepted(): boolean {
  try {
    localStorage.setItem(AV_DISCLOSURE_KEY, encodeDisclosure());
    return true;
  } catch {
    return false;
  }
}

/** `m:ss` (or `h:mm:ss` from an hour); unknown or negative times show as `0:00`. */
export function formatClock(seconds: number): string {
  const total = Number.isFinite(seconds) && seconds > 0 ? Math.floor(seconds) : 0;
  const s = String(total % 60).padStart(2, "0");
  const minutes = Math.floor(total / 60);
  if (minutes < 60) return `${minutes}:${s}`;
  return `${Math.floor(minutes / 60)}:${String(minutes % 60).padStart(2, "0")}:${s}`;
}

/** The index of the candidate to try after `index` failed, or `null` when none is left. */
export function nextCandidate(index: number, count: number): number | null {
  return index + 1 < count ? index + 1 : null;
}

/** The slider position as a fraction of the length, clamped to 0..1. */
export function positionFraction(position: number, duration: number): number {
  if (!Number.isFinite(position) || !Number.isFinite(duration) || duration <= 0) return 0;
  return Math.min(1, Math.max(0, position / duration));
}

/** Only addresses on Wikimedia's media host are ever given to the media element. */
export function isStreamAddress(url: string): boolean {
  try {
    const parsed = new URL(url);
    return parsed.protocol === "https:" && parsed.hostname === "upload.wikimedia.org"
      && parsed.port === "" && parsed.username === "" && parsed.search === "" && parsed.hash === "";
  } catch {
    return false;
  }
}

export type AvStreamInfo = { title: string; video: boolean; candidates: string[] };

/** The command's answer if it has the expected shape; otherwise `null`. */
export function parseAvStreamInfo(value: unknown): AvStreamInfo | null {
  if (typeof value !== "object" || value === null) return null;
  const { title, video, candidates } = value as Record<string, unknown>;
  if (typeof title !== "string" || typeof video !== "boolean" || !Array.isArray(candidates)) return null;
  if (candidates.length === 0 || !candidates.every((c) => typeof c === "string" && isStreamAddress(c))) return null;
  return { title, video, candidates: candidates as string[] };
}

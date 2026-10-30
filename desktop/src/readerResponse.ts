/** Validate untrusted Tauri IPC results before rendering or navigation. */
export type ReaderSearchHit = [pageId: number, title: string];
export type ReaderArticle = [pageId: number, revisionId: number, title: string, html: string];
export type ReaderBookmark = [project: string, pageId: number, revisionId: number];
export type ReaderVisit = [project: string, pageId: number, revisionId: number, visitedAt: number];
const SUPPORTED_PROJECTS = new Set(["enwiki", "dewiki", "frwiki", "eswiki", "jawiki"]);

const MAX_HTML_BYTES = 2_000_000;

function validId(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value > 0;
}

/** Match wiki-model's first-Unicode-scalar title key (not locale-wide case folding). */
export function navigationTitleKey(title: string): string {
  const spaced = title.replaceAll("_", " ");
  const scalars = [...spaced];
  if (!scalars.length) return spaced;
  const upper = scalars[0].toUpperCase();
  // A multi-scalar uppercase expansion must not change the first character.
  return [...upper].length === 1 ? upper + scalars.slice(1).join("") : spaced;
}

/** Reject malformed UTF-16: TextEncoder silently replaces unpaired surrogates,
 * which could change a Wikipedia page identity between UI and transport. */
export function wellFormedTitleUnicode(value: string): boolean {
  for (let index = 0; index < value.length; index++) {
    const unit = value.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      if (index + 1 === value.length) return false;
      const next = value.charCodeAt(index + 1);
      if (next < 0xdc00 || next > 0xdfff) return false;
      index++;
    } else if (unit >= 0xdc00 && unit <= 0xdfff) {
      return false;
    }
  }
  return true;
}

/** Mirror the Rust navigation boundary: invisible format controls and Unicode
 * noncharacters must never be accepted as page identities. */
export function unsafeNavigationUnicode(value: string): boolean {
  for (const character of value) {
    const codepoint = character.codePointAt(0)!;
    if ((codepoint <= 0x1f || (codepoint >= 0x7f && codepoint <= 0x9f)) ||
        (codepoint >= 0xfdd0 && codepoint <= 0xfdef) ||
        (codepoint & 0xfffe) === 0xfffe ||
        codepoint === 0x061c || codepoint === 0x200b ||
        codepoint === 0x200e || codepoint === 0x200f ||
        (codepoint >= 0x2028 && codepoint <= 0x202e) ||
        codepoint === 0x2060 ||
        (codepoint >= 0x2066 && codepoint <= 0x206f) ||
        codepoint === 0xfeff) return true;
  }
  return false;
}

function validTitle(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= 512 &&
    value === value.trim() && wellFormedTitleUnicode(value) && !unsafeNavigationUnicode(value);
}

/** Validate a title before a user-initiated exact-title fetch.
 * Unlike IPC display labels, Action API title requests also have a 512-byte
 * UTF-8 limit; reject oversize multi-byte titles before invoking the backend.
 */
export function validReaderTitleInput(value: unknown): value is string {
  return validTitle(value) && new TextEncoder().encode(value).byteLength <= 512;
}

/** Match the Rust online search input limit before sending an IPC request.
 * Trim UI-only outer whitespace, but never silently rewrite internal controls
 * or malformed Unicode into a different query on the wire. */
export function normalizedReaderSearchQuery(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const normalized = value.trim();
  return validReaderTitleInput(normalized) ? normalized : null;
}

export function parseReaderSearchHits(input: unknown): ReaderSearchHit[] {
  if (!Array.isArray(input) || input.length > 100) throw new Error("Invalid Wikipedia search response");
  const seen = new Set<number>();
  const seenTitles = new Set<string>();
  // Array.from visits sparse slots; Array.prototype.map silently skips them.
  return Array.from(input, (item: unknown): ReaderSearchHit => {
    if (!Array.isArray(item) || item.length !== 2 || !validId(item[0]) ||
        !validTitle(item[1]) || seen.has(item[0])) {
      throw new Error("Invalid Wikipedia search identity");
    }
    const titleKey = navigationTitleKey(item[1]);
    if (seenTitles.has(titleKey)) throw new Error("Conflicting Wikipedia search titles");
    seen.add(item[0]);
    seenTitles.add(titleKey);
    return [item[0], item[1]];
  });
}

export function parseReaderArticle(input: unknown, expectedPageId?: number): ReaderArticle {
  if (!Array.isArray(input) || input.length !== 4 ||
      !validId(input[0]) || !validId(input[1]) || !validTitle(input[2]) ||
      typeof input[3] !== "string" || input[3].length > MAX_HTML_BYTES ||
      new TextEncoder().encode(input[3]).byteLength > MAX_HTML_BYTES ||
      (expectedPageId !== undefined && input[0] !== expectedPageId)) {
    throw new Error("Invalid Wikipedia article identity or preview");
  }
  return [input[0], input[1], input[2], input[3]];
}

/** Reject malformed or duplicate persisted bookmark IPC rows before display.
 * Bookmarks identify pages only, never complete offline content.
 */
export function parseReaderBookmarks(value: unknown): ReaderBookmark[] {
  if (!Array.isArray(value) || value.length > 100) throw new Error("Invalid bookmark list");
  const seen = new Set<string>();
  return Array.from(value, (row: unknown): ReaderBookmark => {
    if (!Array.isArray(row) || row.length !== 3 ||
        typeof row[0] !== "string" || !SUPPORTED_PROJECTS.has(row[0]) ||
        !validId(row[1]) || !validId(row[2])) {
      throw new Error("Invalid bookmark identity");
    }
    const identity = row[0] + ":" + row[1];
    if (seen.has(identity)) throw new Error("Duplicate bookmark identity");
    seen.add(identity);
    return [row[0], row[1], row[2]];
  });
}

/** A bounded list of local, opt-in visits. Repeated page visits are valid. */
export function parseReaderVisits(value: unknown): ReaderVisit[] {
  if (!Array.isArray(value) || value.length > 50) throw new Error("Invalid reading history list");
  return Array.from(value, (row: unknown): ReaderVisit => {
    if (!Array.isArray(row) || row.length !== 4 ||
        typeof row[0] !== "string" || !SUPPORTED_PROJECTS.has(row[0]) ||
        !validId(row[1]) || !validId(row[2]) ||
        typeof row[3] !== "number" || !Number.isSafeInteger(row[3]) ||
        row[3] <= 0 || row[3] > 253402300799) {
      throw new Error("Invalid reading history identity");
    }
    return [row[0], row[1], row[2], row[3]];
  });
}

/** Validate all three saved-content lists as one native privacy-generation-fenced
 * response. Never display a partially decoded saved panel. */
export function parseReaderSavedPanel(value: unknown): [ReaderBookmark[], boolean, ReaderVisit[]] {
  if (!Array.isArray(value) || value.length !== 3 || typeof value[1] !== "boolean") {
    throw new Error("Invalid saved content panel");
  }
  return [parseReaderBookmarks(value[0]), value[1], parseReaderVisits(value[2])];
}

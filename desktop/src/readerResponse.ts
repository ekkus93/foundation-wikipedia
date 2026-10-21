/** Validate untrusted Tauri IPC results before rendering or navigation. */
export type ReaderSearchHit = [pageId: number, title: string];
export type ReaderArticle = [pageId: number, revisionId: number, title: string, html: string];

const MAX_HTML_BYTES = 2_000_000;

function validId(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value > 0;
}

function validTitle(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= 512 &&
    value === value.trim() && !/[\u0000-\u001f\u007f-\u009f\u202a-\u202e\u2066-\u2069]/u.test(value);
}

export function parseReaderSearchHits(input: unknown): ReaderSearchHit[] {
  if (!Array.isArray(input) || input.length > 100) throw new Error("Invalid Wikipedia search response");
  const seen = new Set<number>();
  return input.map((item: unknown): ReaderSearchHit => {
    if (!Array.isArray(item) || item.length !== 2 || !validId(item[0]) ||
        !validTitle(item[1]) || seen.has(item[0])) {
      throw new Error("Invalid Wikipedia search identity");
    }
    seen.add(item[0]);
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

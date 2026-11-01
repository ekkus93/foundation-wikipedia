/** Strictly decode read-only, revision-scoped chat history returned by native SQLite IPC.
 * SQLite i64 IDs are accepted only while exactly representable in JavaScript.
 * Historical assistant text is plain text, NOT a verified citation or trusted HTML. */
export type ReaderChatExchange = [questionId: number, answerId: number, question: string, answer: string];
const MAX_MESSAGE_BYTES = 65_536;
const encoder = new TextEncoder();

function validRowId(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value > 0;
}

function validMessage(value: unknown): value is string {
  if (typeof value !== "string" || value.trim().length === 0 ||
      encoder.encode(value).byteLength > MAX_MESSAGE_BYTES) return false;
  // TextEncoder silently substitutes U+FFFD for malformed UTF-16; reject it.
  for (let i = 0; i < value.length; i++) {
    const code = value.charCodeAt(i);
    if (code >= 0xd800 && code <= 0xdbff) {
      if (++i === value.length) return false;
      const low = value.charCodeAt(i);
      if (low < 0xdc00 || low > 0xdfff) return false;
    } else if (code >= 0xdc00 && code <= 0xdfff) return false;
  }
  return true;
}

export function parseReaderChatThreads(value: unknown): number[] {
  if (!Array.isArray(value) || value.length > 50) throw new Error("Invalid saved chat thread list");
  const seen = new Set<number>();
  return Array.from(value, (id: unknown): number => {
    if (!validRowId(id) || seen.has(id)) throw new Error("Invalid saved chat thread identity");
    seen.add(id);
    return id;
  });
}

export function parseReaderChatExchanges(value: unknown): ReaderChatExchange[] {
  if (!Array.isArray(value) || value.length > 20) throw new Error("Invalid saved chat exchange list");
  let lastAnswerId = 0;
  return Array.from(value, (row: unknown): ReaderChatExchange => {
    if (!Array.isArray(row) || row.length !== 4 ||
        !validRowId(row[0]) || !validRowId(row[1]) ||
        row[0] <= lastAnswerId || row[1] <= row[0] ||
        !validMessage(row[2]) || !validMessage(row[3])) {
      throw new Error("Invalid saved chat exchange");
    }
    lastAnswerId = row[1];
    return [row[0], row[1], row[2], row[3]];
  });
}

/** A delete response belongs only to the article and thread that initiated it.
 * Invalidate on navigation or unmount so an old IPC completion cannot erase
 * or overwrite the next article's conversation list (even if IDs repeat). */
class ReaderChatIdentityFence {
  private epoch = 0;
  private active: { epoch: number; identity: string; threadId: number } | null = null;

  begin(article: { project: string; pageId: number; revisionId: number }, threadId: number): number {
    const epoch = ++this.epoch;
    this.active = { epoch, identity: JSON.stringify([article.project, article.pageId, article.revisionId]), threadId };
    return epoch;
  }

  isCurrent(epoch: number, article: { project: string; pageId: number; revisionId: number }, threadId: number): boolean {
    return this.active?.epoch === epoch && this.active.threadId === threadId &&
      this.active.identity === JSON.stringify([article.project, article.pageId, article.revisionId]);
  }

  invalidate(): void {
    this.epoch += 1;
    this.active = null;
  }
}

/** Independent identity generations for native deletes and transcript reads.
 * A new thread selection invalidates older exchange reads before effect cleanup. */
export class ReaderChatDeleteFence extends ReaderChatIdentityFence {}
export class ReaderChatExchangeReadFence extends ReaderChatIdentityFence {}

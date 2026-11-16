/** UX-002 desktop article chat: validated shapes of the native chat DTOs.
 * Everything from IPC is checked before display; citations are numbered by
 * first appearance and point only to passages the native side validated. */

export interface ChatPassage {
  blockId: string;
  headingPath: string[];
  excerpt: string;
}

export interface ChatPreview {
  id: number;
  destination: string;
  locality: "this device" | "local network" | "internet";
  model: string;
  profile: string;
  articleTitle: string;
  revisionId: number;
  passages: ChatPassage[];
  selectedPassage: ChatPassage | null;
  /** The exchange will be saved to the private history. */
  saved: boolean;
}

export type AnswerPart = { kind: "text"; text: string } | { kind: "citation"; passage: ChatPassage };
export type ChatAnswer = { kind: "supported"; parts: AnswerPart[] } | { kind: "insufficient_evidence" };

export interface ChatProgress {
  id: number;
  elapsedMs: number;
  firstOutputMs: number | null;
  finished: boolean;
  draft: string;
}

/** One answered question, kept for the article while the window is closed. */
export interface ChatExchange {
  question: string;
  answer: ChatAnswer;
}

const fail = (): never => { throw new Error("Invalid chat data"); };
const record = (value: unknown): Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value) ? value as Record<string, unknown> : fail();
const text = (value: unknown, max = 100_000): string =>
  typeof value === "string" && value.length <= max ? value : fail();
const count = (value: unknown): number =>
  typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : fail();

export function parsePassage(value: unknown): ChatPassage {
  const item = record(value);
  const headings = Array.isArray(item.heading_path) && item.heading_path.length <= 32
    ? item.heading_path.map((heading) => text(heading, 512)) : fail();
  const blockId = text(item.block_id, 256);
  if (!/^wkb:[a-z0-9_]+:\d+:\d+:b\d+$/u.test(blockId)) fail();
  return { blockId, headingPath: headings, excerpt: text(item.excerpt, 20_000) };
}

export function parseChatPreview(value: unknown): ChatPreview {
  const item = record(value);
  const locality = item.locality;
  if (locality !== "this device" && locality !== "local network" && locality !== "internet") fail();
  const passages = Array.isArray(item.passages) && item.passages.length <= 64 ? item.passages.map(parsePassage) : fail();
  return {
    id: count(item.id),
    destination: text(item.destination, 512),
    locality: locality as ChatPreview["locality"],
    model: text(item.model, 256),
    profile: text(item.profile, 64),
    articleTitle: text(item.article_title, 1024),
    revisionId: count(item.revision_id),
    passages,
    selectedPassage: item.selected_passage == null ? null : parsePassage(item.selected_passage),
    saved: item.saved === true,
  };
}

export function parseChatAnswer(value: unknown): ChatAnswer {
  const item = record(value);
  if (item.kind === "insufficient_evidence") return { kind: "insufficient_evidence" };
  if (item.kind !== "supported" || !Array.isArray(item.parts) || item.parts.length > 4096) fail();
  return {
    kind: "supported",
    parts: (item.parts as unknown[]).map((part): AnswerPart => {
      const entry = record(part);
      if (entry.kind === "text") return { kind: "text", text: text(entry.text) };
      if (entry.kind === "citation") return { kind: "citation", passage: parsePassage(entry.passage) };
      return fail();
    }),
  };
}

/** A sent question's result and its saved conversation, if any. */
export function parseChatReply(value: unknown): { answer: ChatAnswer; threadId: number | null } {
  const item = record(value);
  const threadId = item.thread_id == null ? null : count(item.thread_id);
  return { answer: parseChatAnswer(item.answer), threadId };
}

export function parseChatProgress(value: unknown): ChatProgress | null {
  try {
    const item = record(value);
    return {
      id: count(item.id),
      elapsedMs: count(item.elapsed_ms),
      firstOutputMs: item.first_output_ms == null ? null : count(item.first_output_ms),
      finished: item.finished === true,
      draft: text(item.draft, 65_536),
    };
  } catch {
    return null;
  }
}

/** A citation as the reader shows it: number by first appearance. */
export interface NumberedCitation {
  number: number;
  passage: ChatPassage;
}

export type DisplaySegment = { kind: "text"; text: string } | { kind: "citation"; number: number };

/** Answer text with `[n]` markers, plus the distinct cited passages. */
export function numberCitations(parts: AnswerPart[]): { segments: DisplaySegment[]; citations: NumberedCitation[] } {
  const numbers = new Map<string, number>();
  const citations: NumberedCitation[] = [];
  const segments = parts.map((part): DisplaySegment => {
    if (part.kind === "text") return part;
    let number = numbers.get(part.passage.blockId);
    if (number === undefined) {
      number = numbers.size + 1;
      numbers.set(part.passage.blockId, number);
      citations.push({ number, passage: part.passage });
    }
    return { kind: "citation", number };
  });
  return { segments, citations };
}

/** Label shown on a streamed draft before its citations are checked. */
export const DRAFT_LABEL = "Draft – citations not checked yet";

/** The Wikipedia edition new reader tabs start with (Settings › General). */
/** The Wikipedia editions the reader supports: the one list every module
 * uses (IPC validation, links, source URLs, the edition picker). */
export const EDITIONS = [
  ["enwiki", "English"], ["simplewiki", "Simple English"], ["dewiki", "Deutsch"],
  ["frwiki", "Français"], ["eswiki", "Español"], ["jawiki", "日本語"],
] as const;
export type Edition = (typeof EDITIONS)[number][0];
export const EDITION_IDS: ReadonlySet<string> = new Set(EDITIONS.map(([id]) => id));

/** `enwiki` → `en.wikipedia.org`, `simplewiki` → `simple.wikipedia.org`. */
export function editionHost(edition: Edition): string {
  return `${edition.slice(0, -"wiki".length)}.wikipedia.org`;
}

const KEY = "reader.defaultEdition.v1";

export function isEdition(value: unknown): value is Edition {
  return typeof value === "string" && EDITIONS.some(([id]) => id === value);
}

/** The saved default, or English; storage problems never break the reader. */
export function loadDefaultEdition(): Edition {
  try {
    const saved = localStorage.getItem(KEY);
    return isEdition(saved) ? saved : "enwiki";
  } catch {
    return "enwiki";
  }
}

export function saveDefaultEdition(edition: Edition): boolean {
  try {
    localStorage.setItem(KEY, edition);
    return true;
  } catch {
    return false;
  }
}

/** The Wikipedia edition new reader tabs start with (Settings › General). */
export const EDITIONS = [
  ["enwiki", "English"], ["dewiki", "Deutsch"], ["frwiki", "Français"], ["eswiki", "Español"], ["jawiki", "日本語"],
] as const;
export type Edition = (typeof EDITIONS)[number][0];

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

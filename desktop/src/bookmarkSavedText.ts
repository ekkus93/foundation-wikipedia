/** "Saved 26 Nov 2026" for a bookmark with a saved time (Unix seconds, UTC);
 * "" for one saved before dates were kept, so nothing is shown. The date is
 * formatted with the platform's locale. `format` is injectable for tests. */
export function bookmarkSavedText(
  savedAt: number | null,
  format: (date: Date) => string = (date) =>
    date.toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" }),
): string {
  if (savedAt === null || !Number.isSafeInteger(savedAt) || savedAt <= 0) return "";
  return `Saved ${format(new Date(savedAt * 1000))}`;
}

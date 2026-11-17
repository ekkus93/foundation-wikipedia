import { EDITIONS, editionHost } from "./defaultEdition.ts";
/** Revision-pinned source URLs for allowlisted Wikimedia projects only. */
const WIKI_HOSTS: Readonly<Record<string, string>> = Object.freeze(
  Object.fromEntries(EDITIONS.map(([id]) => [id, editionHost(id)])));
export function revisionSourceUrl(project: unknown, pageId: unknown, revisionId: unknown): string | null {
  if (typeof project !== "string" || !Object.hasOwn(WIKI_HOSTS, project) ||
      typeof pageId !== "number" || !Number.isSafeInteger(pageId) || pageId <= 0 ||
      typeof revisionId !== "number" || !Number.isSafeInteger(revisionId) || revisionId <= 0) {
    return null;
  }
  const url = new URL(`https://${WIKI_HOSTS[project]}/w/index.php`);
  url.searchParams.set("oldid", String(revisionId));
  return url.toString();
}

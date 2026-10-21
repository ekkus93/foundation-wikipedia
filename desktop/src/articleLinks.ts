/** Fail-closed navigation for untrusted Wikimedia article HTML.
 * The isolated preview iframe remains sandboxed with scripts and clicks disabled.
 * Only validated, same-edition mainspace titles reach the existing Rust adapter.
 */
export interface WikiArticleLink {
  title: string;
  label: string;
}

const EDITIONS = new Set(["enwiki", "dewiki", "frwiki", "eswiki", "jawiki"]);
const MAX_LINKS = 80;
const MAX_HTML_BYTES = 2_000_000;

export function normalizeWikiHref(href: string, project: string): string | null {
  if (!EDITIONS.has(project) || !href || href.length > 2048 || /[\\\\\u0000-\u001f\u007f-\u009f\u061c\u200e-\u200f\u202a-\u202e\u2066-\u2069]/u.test(href) || href.trim() !== href) return null;
  // Do not interpret arbitrary relative URLs, schemes, query-only links or anchors.
  if (!/^(?:\.\/|\/wiki\/|https:\/\/|\/\/)/u.test(href)) return null;
  // WHATWG URL parsing removes dot segments before callers can inspect them.
  // Reject encoded path separators and dot segments in the original href so
  // untrusted markup cannot disguise a different article as its destination.
  // Match arbitrarily nested percent escapes, not only double encoding.
  const rawPath = href.startsWith("./") ? href.slice(2).split(/[?#]/u, 1)[0] : href.split(/[?#]/u, 1)[0];
  if (/%(?:25)*(?:2f|5c)|%(?:25)+(?:2e)/iu.test(rawPath) || rawPath.split("/").some((segment) => /^(?:\.|%2e){1,2}$/iu.test(segment))) return null;
  const lang = project.slice(0, -4);
  let url: URL;
  try {
    url = new URL(href, `https://${lang}.wikipedia.org/wiki/`);
  } catch {
    return null;
  }
  if (url.protocol !== "https:" || url.hostname !== `${lang}.wikipedia.org` || url.username || url.password || url.port || url.search) return null;
  if (!url.pathname.startsWith("/wiki/")) return null;
  const rawTitle = url.pathname.slice("/wiki/".length);
  if (!rawTitle || rawTitle.length > 1536) return null;
  let title: string;
  try {
    title = decodeURIComponent(rawTitle).replaceAll("_", " ");
  } catch {
    return null;
  }
  // A colon indicates a namespace/interwiki prefix, not an article link.
  if (!title || title.length > 512 || title.trim() !== title || /[:?#\\\\\u0000-\u001f\u007f-\u009f\u061c\u200e-\u200f\u202a-\u202e\u2066-\u2069]/u.test(title)) return null;
  if (title.split("/").some((segment) => segment === "." || segment === "..")) return null;
  return title;
}

/** Parse untrusted HTML in a detached template, not DOMParser.
 * DOMParser documents can fetch img/iframe resources even when scripts are inert.
 * Template contents stay uninstantiated and must never be appended to the DOM.
 */
export function extractWikiArticleLinks(html: string, project: string): WikiArticleLink[] {
  // The budget is in UTF-8 bytes, not UTF-16 code units; non-ASCII HTML can exceed it.
  if (!EDITIONS.has(project) || html.length > MAX_HTML_BYTES) return [];
  if (new TextEncoder().encode(html).byteLength > MAX_HTML_BYTES) return [];
  const template = document.createElement("template");
  template.innerHTML = html;
  const links: WikiArticleLink[] = [];
  const seen = new Set<string>();
  for (const anchor of template.content.querySelectorAll("a[href]")) {
    const title = normalizeWikiHref(anchor.getAttribute("href") ?? "", project);
    if (!title || seen.has(title)) continue;
    seen.add(title);
    const label = (anchor.textContent ?? "")
      .replace(/[\u0000-\u001f\u007f-\u009f]/gu, " ")
      .replace(/[\u061c\u200e-\u200f\u202a-\u202e\u2066-\u2069]/gu, "")
      .trim().replace(/\s+/gu, " ").slice(0, 160) || title;
    links.push({ title, label });
    if (links.length >= MAX_LINKS) break;
  }
  return links;
}

import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { RequestEpoch } from "./requestEpoch";
import { backReaderArticle, emptyReaderNavigation, forwardReaderArticle, mountedReaderFrames, openReaderArticle, type ReaderNavigation } from "./readerHistory";
import { extractWikiArticleLinks, type WikiArticleLink } from "./articleLinks";
import { parseReaderArticle, parseReaderSearchHits, validReaderTitleInput, type ReaderSearchHit } from "./readerResponse";


export interface ReaderArticleContext {
  project: string;
  pageId: number;
  revisionId: number;
}

interface ArticlePreview {
  frameId: number;
  pageId: number;
  revisionId: number;
  title: string;
  html: string;
  links: WikiArticleLink[];
}

// The iframe is deliberately unprivileged and has no external resource access.
// Required visuals are not yet installed, so this is never labeled offline-ready.
function isolatedDocument(html: string): string {
  return [
    "<!doctype html><html><head>",
    "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src 'none'; font-src 'none'; connect-src 'none'; media-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none'; style-src 'unsafe-inline'\">",
    "<meta name=\"referrer\" content=\"no-referrer\">",
    "<style>body{font:17px/1.6 Georgia,serif;padding:1rem;margin:0;color:#202122}table{max-width:100%;overflow-wrap:anywhere}img,video,audio{display:none}a{pointer-events:none}pre{white-space:pre-wrap}*{max-width:100%}</style>",
    "</head><body>",
    html,
    "</body></html>",
  ].join("");
}

export default function OnlineReader({ onArticleChange }: { onArticleChange?: (title: string | null, context: ReaderArticleContext | null) => void }) {
  const uniqueId = useId();
  const projectFieldId = `${uniqueId}-project`;
  const queryFieldId = `${uniqueId}-query`;
  const [query, setQuery] = useState("");
  const [project, setProject] = useState("enwiki");
  const [offline, setOffline] = useState(false);
  const [connected, setConnected] = useState(() => navigator.onLine);
  const requestEpoch = useRef(new RequestEpoch());
  const nextFrameId = useRef(1);
  const [hits, setHits] = useState<ReaderSearchHit[]>([]);
  const [navigation, setNavigation] = useState<ReaderNavigation<ArticlePreview>>(() => emptyReaderNavigation());
  const article = navigation.current;
  const mountedFrames = mountedReaderFrames(navigation);
  useEffect(() => {
    onArticleChange?.(
      article?.title ?? null,
      article ? { project, pageId: article.pageId, revisionId: article.revisionId } : null,
    );
  }, [article?.title, article?.pageId, article?.revisionId, project, onArticleChange]);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    const disconnected = () => {
      requestEpoch.current.invalidate();
      setConnected(false);
      setBusy(false);
      setHits([]);
      // Preserve already-rendered, sandboxed text snapshots across a disconnect.
      // They remain incomplete online previews, never installed offline articles.
      setNotice("Connection lost. Previously loaded online text previews remain readable, but no complete offline article is installed.");
    };
    const reconnected = () => setConnected(true);
    window.addEventListener("offline", disconnected);
    window.addEventListener("online", reconnected);
    return () => {
      window.removeEventListener("offline", disconnected);
      window.removeEventListener("online", reconnected);
    };
  }, []);

  async function search(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!query.trim() || busy || offline || !connected || !navigator.onLine) return;
    const epoch = requestEpoch.current.begin();
    setBusy(true);
    setNotice(null);
    // A failed search must not destroy the currently displayed revision/history.
    try {
      const result = parseReaderSearchHits(await invoke<unknown>("search_wikipedia", { query: query.trim(), project, offline: offline || !connected || !navigator.onLine }));
      if (!requestEpoch.current.matches(epoch)) return;
      setHits(result);
      setNavigation(emptyReaderNavigation());
      if (!result.length) setNotice("No Wikipedia articles matched that search.");
    } catch (error) {
      if (requestEpoch.current.matches(epoch)) {
        setNotice(`Search unavailable: ${String(error)}`);
      }
    } finally {
      if (requestEpoch.current.matches(epoch)) setBusy(false);
    }
  }

  async function openArticle(command: "open_wikipedia_title" | "open_wikipedia_page", args: { title?: string; pageId?: number }) {
    if (busy || offline || !connected || !navigator.onLine) return;
    const epoch = requestEpoch.current.begin();
    setBusy(true);
    setNotice(null);
    // Keep the current article visible if the next fetch fails.
    try {
      const [pageId, revisionId, canonicalTitle, html] = parseReaderArticle(await invoke<unknown>(
        command,
        { ...args, project, offline: offline || !connected || !navigator.onLine },
      ), args.pageId);
      if (!requestEpoch.current.matches(epoch)) return;
      setNavigation((state) => openReaderArticle(state, { frameId: nextFrameId.current++, pageId, revisionId, title: canonicalTitle, html, links: extractWikiArticleLinks(html, project) }));
    } catch (error) {
      if (requestEpoch.current.matches(epoch)) setNotice(`Article unavailable: ${String(error)}`);
    } finally {
      if (requestEpoch.current.matches(epoch)) setBusy(false);
    }
  }

  function openTitle(title: string) {
    if (!validReaderTitleInput(title)) {
      setNotice("Invalid Wikipedia article title. Enter a mainspace title without control characters.");
      return;
    }
    return openArticle("open_wikipedia_title", { title });
  }

  function openPage(pageId: number) {
    if (!Number.isSafeInteger(pageId) || pageId <= 0) {
      setNotice("Invalid Wikipedia page identity in search results.");
      return;
    }
    return openArticle("open_wikipedia_page", { pageId });
  }

  return (
    <section aria-label="Online Wikipedia reader">
      <form className="online-search" onSubmit={search}>
        <label htmlFor={projectFieldId}>Wikipedia edition</label>
        <select id={projectFieldId} value={project} disabled={busy} onChange={(event) => {
          setProject(event.target.value);
          setHits([]);
          setNavigation(emptyReaderNavigation());
          setNotice(null);
        }}>
          <option value="enwiki">English</option>
          <option value="dewiki">Deutsch</option>
          <option value="frwiki">Français</option>
          <option value="eswiki">Español</option>
          <option value="jawiki">日本語</option>
        </select>
        <label htmlFor={queryFieldId}>Search articles</label>
        <div>
          <input
            id={queryFieldId}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            maxLength={512}
            placeholder="Article or topic"
            disabled={busy}
          />
          <button type="submit" disabled={busy || offline || !connected || !navigator.onLine || !query.trim()}>Search</button>
          <button type="button" disabled={busy || offline || !connected || !navigator.onLine || !query.trim()}
            onClick={() => void openTitle(query.trim())}>Open exact title</button>
        </div>
      </form>
      <label><input type="checkbox" checked={offline} disabled={busy} onChange={(event) => {
        const nowOffline = event.target.checked;
        setOffline(nowOffline);
        if (nowOffline) {
          // Invalidate pending online results, but retain isolated in-memory
          // revision snapshots for Back/Forward. Never label them offline-ready.
          requestEpoch.current.invalidate();
          setHits([]);
          setNotice("Offline mode enabled. Previously loaded online text previews remain readable, but no complete offline article is installed.");
        } else {
          setNotice(null);
        }
      }} /> Work offline (no network requests)</label>
      <p className="preview-warning">
        {offline || !connected ? "Network disabled: only previously loaded in-memory text previews can be read; complete offline articles require an installed pack. " : ""}
        Online text preview only. Images, equations, and other required media are not saved;
        this page is not offline-ready. Links inside the isolated preview are disabled; verified same-edition article links are listed below.
      </p>
      {notice && <p role="status">{notice}</p>}
      {busy && <p role="status">Loading Wikipedia…</p>}
      {!article && navigation.forward.length > 0 && <button type="button" disabled={busy} onClick={() => setNavigation(forwardReaderArticle)}>→ Forward to article</button>}
      {article && (
        <div className="online-article">
          <button type="button" disabled={busy} onClick={() => {
            setNavigation(backReaderArticle);
            setNotice(null);
          }}>← {navigation.back.length ? "Back to previous article" : "Back to search results"}</button>
          {navigation.forward.length > 0 && <button type="button" disabled={busy} onClick={() => {
            setNavigation(forwardReaderArticle);
            setNotice(null);
          }}>→ Forward to article</button>}
          <h2>{article.title}</h2>
          <p>{offline || !connected ? "Retained online text preview" : "Online revision"} {article.revisionId} · page ID {article.pageId}</p>
        </div>
      )}
      {/* Hidden sandboxed iframes remain mounted for nearby navigation history.
          Never add allow-same-origin/allow-scripts just to read their scroll. */}
      <div className="article-preview-frames" hidden={!article}>
        {mountedFrames.map((snapshot) => (
          <iframe
            key={snapshot.frameId}
            title={`Sanitized Wikipedia article: ${snapshot.title}`}
            hidden={!article || snapshot.frameId !== article.frameId}
            sandbox=""
            referrerPolicy="no-referrer"
            className="article-preview-frame"
            srcDoc={isolatedDocument(snapshot.html)}
          />
        ))}
      </div>
      {article && article.links.length > 0 && (
        <nav aria-label="Internal Wikipedia article links">
          <h3>Article links</h3>
          <ul>
            {article.links.map((link) => (
              <li key={link.title}>
                <button type="button" disabled={busy || offline || !connected || !navigator.onLine} onClick={() => void openTitle(link.title)}>
                  {link.label}
                </button>
              </li>
            ))}
          </ul>
        </nav>
      )}
      {!article && hits.length > 0 && (
        <div className="online-results">
          <h2>Search results</h2>
          <ul>
            {hits.map(([pageId, title]) => (
              <li key={pageId}>
                <button type="button" disabled={busy || offline || !connected || !navigator.onLine} onClick={() => void openPage(pageId)}>
                  {title}
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
    </section>
  );
}

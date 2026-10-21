import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { isolatedReaderDocument, type ReaderTextSize, type ReaderWidth } from "./readerLayout";
import { revisionSourceUrl } from "./articleProvenance";
import { RequestEpoch } from "./requestEpoch";
import { backReaderArticle, emptyReaderNavigation, forwardReaderArticle, mountedReaderFrames, openBookmarkedReaderArticle, openReaderArticle, type ReaderNavigation } from "./readerHistory";
import { extractWikiArticleLinks, type WikiArticleLink } from "./articleLinks";
import { parseReaderArticle, parseReaderSearchHits, validReaderTitleInput, type ReaderSearchHit } from "./readerResponse";


export interface ReaderArticleContext {
  project: string;
  pageId: number;
  revisionId: number;
}

export interface ReaderOpenPageRequest {
  id: number;
  project: string;
  pageId: number;
}

interface ArticlePreview {
  frameId: number;
  pageId: number;
  revisionId: number;
  title: string;
  html: string;
  links: WikiArticleLink[];
}

// Isolated preview HTML is built using whitelisted, app-controlled layout choices.
export default function OnlineReader({ onArticleChange, bookmarkRequest, textSize, readingWidth }: {
  onArticleChange?: (title: string | null, context: ReaderArticleContext | null) => void;
  bookmarkRequest?: ReaderOpenPageRequest | null;
  textSize: ReaderTextSize;
  readingWidth: ReaderWidth;
}) {
  const uniqueId = useId();
  const projectFieldId = `${uniqueId}-project`;
  const queryFieldId = `${uniqueId}-query`;
  const [query, setQuery] = useState("");
  const [project, setProject] = useState("enwiki");
  const [offline, setOffline] = useState(false);
  const [connected, setConnected] = useState(() => navigator.onLine);
  const requestEpoch = useRef(new RequestEpoch());
  const nextFrameId = useRef(1);
  const lastBookmarkRequestId = useRef(0);
  const [hits, setHits] = useState<ReaderSearchHit[]>([]);
  const [navigation, setNavigation] = useState<ReaderNavigation<ArticlePreview>>(() => emptyReaderNavigation());
  const article = navigation.current;
  const mountedFrames = mountedReaderFrames(navigation);

  // Record user-visible article navigation only. The native SQLite API remains
  // authoritative: history is disabled by default and never writes until opted in.
  useEffect(() => {
    if (!article) return;
    void invoke<boolean>("record_reader_visit", {
      project, pageId: article.pageId, revisionId: article.revisionId,
    }).catch(() => {
      // History recording cannot prevent navigation or disturb its provenance.
    });
  }, [article?.frameId, project]);
  useEffect(() => {
    onArticleChange?.(
      article?.title ?? null,
      article ? { project, pageId: article.pageId, revisionId: article.revisionId } : null,
    );
  }, [article?.title, article?.pageId, article?.revisionId, project, onArticleChange]);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [copyNotice, setCopyNotice] = useState<string | null>(null);

  // Bookmark reopening is explicit user navigation. Never bypass offline mode,
  // and do not discard the active revision/history on a failed fetch.
  useEffect(() => {
    if (!bookmarkRequest || bookmarkRequest.id <= lastBookmarkRequestId.current) return;
    lastBookmarkRequestId.current = bookmarkRequest.id;
    if (offline || !connected || !navigator.onLine) {
      setNotice("Cannot open this saved page without a connection. The current article is unchanged.");
      return;
    }
    if (!Number.isSafeInteger(bookmarkRequest.pageId) || bookmarkRequest.pageId <= 0 ||
        !["enwiki", "dewiki", "frwiki", "eswiki", "jawiki"].includes(bookmarkRequest.project)) {
      setNotice("Invalid saved Wikipedia page identity.");
      return;
    }
    const epoch = requestEpoch.current.begin();
    setBusy(true);
    setNotice(null);
    void invoke<unknown>("open_wikipedia_page", {
      project: bookmarkRequest.project,
      pageId: bookmarkRequest.pageId,
      offline: false,
    }).then((result) => {
      const [pageId, revisionId, title, html] = parseReaderArticle(result, bookmarkRequest.pageId);
      if (!requestEpoch.current.matches(epoch)) return;
      const next = { frameId: nextFrameId.current++, pageId, revisionId, title, html,
        links: extractWikiArticleLinks(html, bookmarkRequest.project) };
      setNavigation((previous) => openBookmarkedReaderArticle(previous, project, bookmarkRequest.project, next));
      setProject(bookmarkRequest.project);
      setHits([]);
      setNotice("Opened the latest available online revision. This is not a complete offline article.");
    }).catch(() => {
      if (requestEpoch.current.matches(epoch)) {
        setNotice("Saved page could not be reopened. The previous article was preserved.");
      }
    }).finally(() => {
      if (requestEpoch.current.matches(epoch)) setBusy(false);
    });
  }, [bookmarkRequest, project, offline, connected]);

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

  async function copyRevisionSource(url: string) {
    try {
      await navigator.clipboard.writeText(url);
      setCopyNotice("Exact-revision source URL copied.");
    } catch {
      setCopyNotice("Clipboard unavailable. Select and copy the revision URL manually.");
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
          {(() => {
            const url = revisionSourceUrl(project, article.pageId, article.revisionId);
            return url && (
              <div className="revision-source">
                <label htmlFor={`${uniqueId}-revision-source`}>Exact Wikimedia revision source</label>
                <input id={`${uniqueId}-revision-source`} type="text" readOnly value={url}
                  aria-description="Source for this exact article revision, not a complete offline license notice." />
                <button type="button" onClick={() => void copyRevisionSource(url)}>Copy source URL</button>
                {copyNotice && <span role="status">{copyNotice}</span>}
              </div>
            );
          })()}
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
            srcDoc={isolatedReaderDocument(snapshot.html, textSize, readingWidth)}
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

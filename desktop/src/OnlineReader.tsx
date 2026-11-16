import { useEffect, useId, useRef, useState, useSyncExternalStore, type FormEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { isolatedReaderDocument, LINK_MESSAGE_KEY, readerDocumentNonce, type ReaderTextSize, type ReaderWidth } from "./readerLayout";
import type { Theme } from "./theme";
import { revisionSourceUrl } from "./articleProvenance";
import { RequestEpoch } from "./requestEpoch";
import { ReaderOfflineMode } from "./readerOfflineMode";
import { backReaderArticle, emptyReaderNavigation, forwardReaderArticle, mountedReaderFrames, openBookmarkedReaderArticle, openReaderArticle, type ReaderNavigation } from "./readerHistory";
import { classifyReaderLink, extractWikiArticleLinks, type WikiArticleLink } from "./articleLinks";
import { navigationTitleKey, normalizedReaderSearchQuery, parseReaderArticle, parseReaderSearchHits, validReaderTitleInput, type ReaderSearchHit } from "./readerResponse";


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
  project: string;
  frameId: number;
  pageId: number;
  revisionId: number;
  title: string;
  html: string;
  links: WikiArticleLink[];
}

// Isolated preview HTML is built using whitelisted, app-controlled layout choices.
export default function OnlineReader({ onArticleChange, bookmarkRequest, textSize, readingWidth, theme, offlineMode }: {
  onArticleChange?: (title: string | null, context: ReaderArticleContext | null) => void;
  bookmarkRequest?: ReaderOpenPageRequest | null;
  textSize: ReaderTextSize;
  readingWidth: ReaderWidth;
  theme: Theme;
  offlineMode: ReaderOfflineMode;
}) {
  const uniqueId = useId();
  const projectFieldId = `${uniqueId}-project`;
  const queryFieldId = `${uniqueId}-query`;
  const [query, setQuery] = useState("");
  const [project, setProject] = useState("enwiki");
  const offline = useSyncExternalStore(offlineMode.subscribe, offlineMode.getSnapshot, offlineMode.getSnapshot);
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
      project: article.project, pageId: article.pageId, revisionId: article.revisionId,
    }).catch(() => {
      // History recording cannot prevent navigation or disturb its provenance.
    });
  }, [article?.frameId, article?.project]);
  useEffect(() => {
    onArticleChange?.(
      article?.title ?? null,
      article ? { project: article.project, pageId: article.pageId, revisionId: article.revisionId } : null,
    );
  }, [article?.title, article?.pageId, article?.revisionId, article?.project, onArticleChange]);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [copyNotice, setCopyNotice] = useState<string | null>(null);
  // DUI-001: each article frame forwards clicked links to this component.
  const frames = useRef(new Map<number, HTMLIFrameElement>());
  const frameNonces = useRef(new Map<number, string>());
  const [externalLink, setExternalLink] = useState<string | null>(null);
  const imagesAllowed = !offline && connected;
  function frameNonce(frameId: number): string {
    let nonce = frameNonces.current.get(frameId);
    if (!nonce) {
      nonce = readerDocumentNonce();
      frameNonces.current.set(frameId, nonce);
    }
    return nonce;
  }

  // Apply the shared offline transition to hidden tabs too. Late responses
  // are ignored; existing in-memory text-only previews remain readable.
  useEffect(() => offlineMode.subscribe(() => {
    if (offlineMode.getSnapshot()) {
      requestEpoch.current.invalidate();
      setBusy(false);
      setHits([]);
      setNotice("Offline mode enabled across all tabs. Previously loaded online text previews remain readable, but no complete offline article is installed.");
    } else {
      setNotice("Offline mode disabled across all tabs. New online actions are available when connected.");
    }
  }), [offlineMode]);

  // Bookmark reopening is explicit user navigation. Never bypass offline mode,
  // and do not discard the active revision/history on a failed fetch.
  useEffect(() => {
    if (!bookmarkRequest || bookmarkRequest.id <= lastBookmarkRequestId.current) return;
    lastBookmarkRequestId.current = bookmarkRequest.id;
    if (offlineMode.getSnapshot() || !connected || !navigator.onLine) {
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
      if (!requestEpoch.current.matches(epoch) || offlineMode.getSnapshot()) return;
      const next = { project: bookmarkRequest.project, frameId: nextFrameId.current++, pageId, revisionId, title, html,
        links: extractWikiArticleLinks(html, bookmarkRequest.project) };
      setNavigation((previous) => openBookmarkedReaderArticle(previous, next));
      setProject(bookmarkRequest.project);
      setHits([]);
      setNotice("Opened the latest available online revision. This is not a complete offline article.");
    }).catch(() => {
      if (requestEpoch.current.matches(epoch) && !offlineMode.getSnapshot()) {
        setNotice("Saved page could not be reopened. The previous article was preserved.");
      }
    }).finally(() => {
      if (requestEpoch.current.matches(epoch)) setBusy(false);
    });
  }, [bookmarkRequest, offline, connected, offlineMode]);

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
    const reconnected = () => {
      setConnected(true);
      setNotice("Connection restored. Online actions are available when Work offline is unchecked.");
    };
    window.addEventListener("offline", disconnected);
    window.addEventListener("online", reconnected);
    return () => {
      window.removeEventListener("offline", disconnected);
      window.removeEventListener("online", reconnected);
    };
  }, []);

  // Links clicked inside the current article frame. Only messages from that
  // exact frame are accepted; the href is untrusted and classified here.
  useEffect(() => {
    if (!article) return;
    const current = article;
    const onMessage = (event: MessageEvent) => {
      const frame = frames.current.get(current.frameId);
      if (!frame || event.source !== frame.contentWindow) return;
      const data: unknown = event.data;
      if (typeof data !== "object" || data === null) return;
      const action = classifyReaderLink((data as Record<string, unknown>)[LINK_MESSAGE_KEY], current.project);
      if (!action) return;
      if (action.kind === "article") {
        if (offlineMode.getSnapshot() || !connected || !navigator.onLine) {
          setNotice("Linked articles cannot be opened while offline. The current article is unchanged.");
          return;
        }
        void openTitle(action.title, current.project);
      } else {
        setExternalLink(action.url);
      }
    };
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  });

  async function openExternalLink(url: string) {
    setExternalLink(null);
    try {
      await invoke("open_external_link", { url });
    } catch {
      setNotice("The link could not be opened in your browser.");
    }
  }

  async function search(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (busy || offlineMode.getSnapshot() || !connected || !navigator.onLine) return;
    const requestedQuery = normalizedReaderSearchQuery(query);
    if (!requestedQuery) {
      setNotice("Invalid search: use a nonempty query of at most 512 UTF-8 bytes without control or invisible formatting characters.");
      return;
    }
    const epoch = requestEpoch.current.begin();
    setBusy(true);
    setNotice(null);
    // A failed search must not destroy the currently displayed revision/history.
    try {
      const result = parseReaderSearchHits(await invoke<unknown>("search_wikipedia", { query: requestedQuery, project, offline: offline || !connected || !navigator.onLine }));
      if (!requestEpoch.current.matches(epoch) || offlineMode.getSnapshot()) return;
      setHits(result);
      setNavigation(emptyReaderNavigation());
      if (!result.length) setNotice("No Wikipedia articles matched that search.");
    } catch (error) {
      if (requestEpoch.current.matches(epoch) && !offlineMode.getSnapshot()) {
        setNotice(`Search unavailable: ${String(error)}`);
      }
    } finally {
      if (requestEpoch.current.matches(epoch)) setBusy(false);
    }
  }

  async function openArticle(command: "open_wikipedia_title" | "open_wikipedia_page", args: { title?: string; pageId?: number }, requestedProject = project) {
    if (busy || offlineMode.getSnapshot() || !connected || !navigator.onLine) return;
    const epoch = requestEpoch.current.begin();
    setBusy(true);
    setNotice(null);
    // Keep the current article visible if the next fetch fails.
    try {
      const [pageId, revisionId, canonicalTitle, html] = parseReaderArticle(await invoke<unknown>(
        command,
        { ...args, project: requestedProject, offline: offline || !connected || !navigator.onLine },
      ), args.pageId);
      if (!requestEpoch.current.matches(epoch) || offlineMode.getSnapshot()) return;
      setNavigation((state) => openReaderArticle(state, { project: requestedProject, frameId: nextFrameId.current++, pageId, revisionId, title: canonicalTitle, html, links: extractWikiArticleLinks(html, requestedProject) }));
      if (args.title && navigationTitleKey(args.title) !== navigationTitleKey(canonicalTitle)) {
        setNotice(`Redirected from “${args.title}”.`);
      }
    } catch (error) {
      if (requestEpoch.current.matches(epoch) && !offlineMode.getSnapshot()) setNotice(`Article unavailable: ${String(error)}`);
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

  function openTitle(title: string, requestedProject = project) {
    if (!validReaderTitleInput(title)) {
      setNotice("Invalid Wikipedia article title. Enter a mainspace title without control characters.");
      return;
    }
    return openArticle("open_wikipedia_title", { title }, requestedProject);
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
          <button type="submit" disabled={busy || offline || !connected || !navigator.onLine || !normalizedReaderSearchQuery(query)}>Search</button>
          <button type="button" disabled={busy || offline || !connected || !navigator.onLine || !normalizedReaderSearchQuery(query)}
            onClick={() => void openTitle(normalizedReaderSearchQuery(query) ?? "")}>Open exact title</button>
        </div>
      </form>
      {query.trim() && !normalizedReaderSearchQuery(query) && (
        <p role="status">Search and exact-title input must fit in 512 UTF-8 bytes and contain no control or invisible formatting characters.</p>
      )}
      <label><input type="checkbox" checked={offline} onChange={(event) => {
        offlineMode.setOffline(event.target.checked);
      }} /> Work offline across all tabs (no new network requests)</label>
      <p className="preview-warning">
        {offline || !connected ? "Network disabled: only previously loaded in-memory text previews can be read; complete offline articles require an installed pack. " : ""}
        Online article. Images and equations load from Wikimedia while online and are not saved;
        this page is not offline-ready. Article links open here; other links ask before opening your browser.
      </p>
      {notice && <p role="status">{notice}</p>}
      {externalLink && (
        <div className="external-link-confirm" role="alertdialog" aria-label="Open link in your browser">
          <p>Open this link in your web browser?</p>
          <p className="external-link-url">{externalLink}</p>
          <button type="button" onClick={() => void openExternalLink(externalLink)}>Open in browser</button>
          <button type="button" autoFocus onClick={() => setExternalLink(null)}>Cancel</button>
        </div>
      )}
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
            const url = revisionSourceUrl(article.project, article.pageId, article.revisionId);
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
          Never add allow-same-origin: article frames must not reach the app. */}
      <div className="article-preview-frames" hidden={!article}>
        {mountedFrames.map((snapshot) => (
          <iframe
            key={snapshot.frameId}
            ref={(element) => {
              if (element) frames.current.set(snapshot.frameId, element);
              else {
                frames.current.delete(snapshot.frameId);
                frameNonces.current.delete(snapshot.frameId);
              }
            }}
            title={`Wikipedia article: ${snapshot.title}`}
            hidden={!article || snapshot.frameId !== article.frameId}
            // Scripts only for the app's nonce'd link forwarder; no same-origin
            // access, popups, forms or top navigation (see readerLayout).
            sandbox="allow-scripts"
            referrerPolicy="no-referrer"
            className="article-preview-frame"
            srcDoc={isolatedReaderDocument(snapshot.html, textSize, readingWidth, theme,
              { images: imagesAllowed, linkNonce: frameNonce(snapshot.frameId) })}
          />
        ))}
      </div>
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

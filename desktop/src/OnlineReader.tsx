import { useEffect, useId, useRef, useState, useSyncExternalStore, type FormEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { isolatedReaderDocument, LINK_MESSAGE_KEY, READY_MESSAGE_KEY, SCROLL_MESSAGE_KEY, SCROLL_TO_KEY, SELECTION_MESSAGE_KEY, type ReaderTextSize, type ReaderWidth } from "./readerLayout";
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
  /** Live Wikipedia, or the active installed pack (chat needs a pack). */
  source: "online" | "pack";
}

/** Open the passage a chat answer cited, in the exact pack revision. */
export interface ReaderCitationRequest {
  id: number;
  project: string;
  pageId: number;
  revisionId: number;
  blockId: string;
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
  /** Where it was read from: live Wikipedia, or the active installed pack. */
  source: "online" | "pack";
}

// Isolated preview HTML is built using whitelisted, app-controlled layout choices.
export default function OnlineReader({ onArticleChange, bookmarkRequest, citationRequest, onSelection, textSize, readingWidth, theme, offlineMode }: {
  onArticleChange?: (title: string | null, context: ReaderArticleContext | null) => void;
  citationRequest?: ReaderCitationRequest | null;
  /** Text the user selected in the visible installed-pack article ("" when none). */
  onSelection?: (text: string) => void;
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
  // Whether the current search results came from the installed pack.
  const [hitsFromPack, setHitsFromPack] = useState(false);
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
      article ? { project: article.project, pageId: article.pageId, revisionId: article.revisionId, source: article.source } : null,
    );
  }, [article?.title, article?.pageId, article?.revisionId, article?.project, article?.source, onArticleChange]);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [copyNotice, setCopyNotice] = useState<string | null>(null);
  // DUI-001: each article frame forwards clicked links to this component.
  const frames = useRef(new Map<number, HTMLIFrameElement>());
  // DUI-003: last scroll position of each article, restored whenever its
  // frame is recreated (older history, theme or offline changes).
  const scrollPositions = useRef(new Map<number, number>());
  const [externalLink, setExternalLink] = useState<string | null>(null);
  const imagesAllowed = !offline && connected;

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
        links: extractWikiArticleLinks(html, bookmarkRequest.project), source: "online" as const };
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

  // A chat citation: open that exact pack revision with the passage
  // highlighted (the frame scrolls it into view).
  const lastCitationRequestId = useRef(0);
  useEffect(() => {
    if (!citationRequest || citationRequest.id <= lastCitationRequestId.current) return;
    lastCitationRequestId.current = citationRequest.id;
    const request = citationRequest;
    const epoch = requestEpoch.current.begin();
    setBusy(true);
    setNotice(null);
    void invoke<unknown>("open_offline_citation", {
      project: request.project, pageId: request.pageId, revisionId: request.revisionId, blockId: request.blockId,
    }).then((result) => {
      if (!requestEpoch.current.matches(epoch)) return;
      if (result === null) {
        setNotice("The cited revision is no longer in the active pack, so the passage cannot be shown.");
        return;
      }
      if (!Array.isArray(result) || result.length !== 5 || typeof result[4] !== "boolean") throw new Error("invalid citation");
      const [pageId, revisionId, title, html] = parseReaderArticle(result.slice(0, 4), request.pageId);
      if (revisionId !== request.revisionId) throw new Error("citation revision changed");
      setNavigation((state) => openReaderArticle(state, {
        project: request.project, frameId: nextFrameId.current++, pageId, revisionId, title, html, links: [], source: "pack",
      }));
      setNotice(result[4] ? "The cited passage is highlighted." : "The cited passage is shown at the top: its exact place in the page could not be found.");
    }).catch(() => {
      if (requestEpoch.current.matches(epoch)) setNotice("The cited passage could not be opened.");
    }).finally(() => {
      if (requestEpoch.current.matches(epoch)) setBusy(false);
    });
  }, [citationRequest]);

  // Messages from article frames. Each is matched to the exact frame that
  // sent it; links are accepted only from the visible article. Data is
  // untrusted and validated here.
  useEffect(() => {
    const current = article;
    const onMessage = (event: MessageEvent) => {
      let frameId: number | null = null;
      for (const [id, frame] of frames.current) {
        if (frame.contentWindow !== null && event.source === frame.contentWindow) frameId = id;
      }
      if (frameId === null) return;
      const data: unknown = event.data;
      if (typeof data !== "object" || data === null) return;
      const record = data as Record<string, unknown>;
      const scroll = record[SCROLL_MESSAGE_KEY];
      if (typeof scroll === "number" && Number.isFinite(scroll) && scroll >= 0 && scroll <= 10_000_000) {
        scrollPositions.current.set(frameId, scroll);
        return;
      }
      const selected = record[SELECTION_MESSAGE_KEY];
      if (typeof selected === "string") {
        if (current && frameId === current.frameId && current.source === "pack" && selected.length <= 2000) {
          onSelection?.(selected);
        }
        return;
      }
      if (record[READY_MESSAGE_KEY] === true) {
        const saved = scrollPositions.current.get(frameId);
        if (saved) frames.current.get(frameId)?.contentWindow?.postMessage({ [SCROLL_TO_KEY]: saved }, "*");
        return;
      }
      if (!current || frameId !== current.frameId) return;
      const action = classifyReaderLink(record[LINK_MESSAGE_KEY], current.project);
      if (!action) return;
      if (action.kind === "article") {
        if (current.source === "pack") {
          void openPackArticle("open_offline_title", { project: current.project, title: action.title });
          return;
        }
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
    if (busy) return;
    const requestedQuery = normalizedReaderSearchQuery(query);
    if (!requestedQuery) {
      setNotice("Invalid search: use a nonempty query of at most 512 UTF-8 bytes without control or invisible formatting characters.");
      return;
    }
    if (offlineMode.getSnapshot() || !connected || !navigator.onLine) {
      await searchPack(requestedQuery);
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
      setHitsFromPack(false);
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

  /** Search the active installed pack (no network). */
  async function searchPack(requestedQuery: string) {
    const epoch = requestEpoch.current.begin();
    setBusy(true);
    setNotice(null);
    try {
      const raw = await invoke<unknown>("search_offline_packs", { query: requestedQuery });
      if (!requestEpoch.current.matches(epoch)) return;
      if (!Array.isArray(raw)) throw new Error("invalid offline search result");
      const sameEdition = raw
        .filter((hit): hit is [string, number, string] => Array.isArray(hit) && hit.length === 3 && hit[0] === project)
        .map(([, pageId, title]) => [pageId, title]);
      const result = parseReaderSearchHits(sameEdition);
      setHits(result);
      setHitsFromPack(true);
      setNavigation(emptyReaderNavigation());
      if (!result.length) setNotice("No articles in the installed pack matched that search.");
    } catch (error) {
      if (requestEpoch.current.matches(epoch)) setNotice(`Offline search unavailable: ${String(error)}`);
    } finally {
      if (requestEpoch.current.matches(epoch)) setBusy(false);
    }
  }

  /** Open an article from the active installed pack (no network). */
  async function openPackArticle(command: "open_offline_page" | "open_offline_title", args: { project: string; pageId?: number; title?: string }) {
    if (busy) return;
    const epoch = requestEpoch.current.begin();
    setBusy(true);
    setNotice(null);
    try {
      const result = await invoke<unknown>(command, args);
      if (!requestEpoch.current.matches(epoch)) return;
      if (result === null) {
        setNotice(args.title ? `“${args.title}” is not in the installed pack.` : "That article is not in the installed pack.");
        return;
      }
      const [pageId, revisionId, title, html] = parseReaderArticle(result, args.pageId);
      setNavigation((state) => openReaderArticle(state, {
        project: args.project, frameId: nextFrameId.current++, pageId, revisionId, title, html, links: [], source: "pack",
      }));
    } catch (error) {
      if (requestEpoch.current.matches(epoch)) setNotice(`Offline article unavailable: ${String(error)}`);
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
      setNavigation((state) => openReaderArticle(state, { project: requestedProject, frameId: nextFrameId.current++, pageId, revisionId, title: canonicalTitle, html, links: extractWikiArticleLinks(html, requestedProject), source: "online" }));
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
    if (offlineMode.getSnapshot() || !connected || !navigator.onLine) {
      return openPackArticle("open_offline_title", { project: requestedProject, title });
    }
    return openArticle("open_wikipedia_title", { title }, requestedProject);
  }

  function openPage(pageId: number) {
    if (!Number.isSafeInteger(pageId) || pageId <= 0) {
      setNotice("Invalid Wikipedia page identity in search results.");
      return;
    }
    if (hitsFromPack) return openPackArticle("open_offline_page", { project, pageId });
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
          <button type="submit" disabled={busy || !normalizedReaderSearchQuery(query)}>Search</button>
          <button type="button" disabled={busy || !normalizedReaderSearchQuery(query)}
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
        {offline || !connected
          ? "Offline: search and articles come from the active installed pack (Settings › Wikipedia & Offline Data). Pack images are not installed yet, so articles show text only. Links to other articles open from the same pack."
          : "Online: articles come from live Wikipedia. Images and equations load from Wikimedia and are not saved, so these pages are not available offline. Article links open here; other links ask before opening your browser."}
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
          <p>{article.source === "pack" ? "Installed pack revision" : offline || !connected ? "Retained online text preview" : "Online revision"} {article.revisionId} · page ID {article.pageId}</p>
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
              else frames.current.delete(snapshot.frameId);
            }}
            title={`Wikipedia article: ${snapshot.title}`}
            hidden={!article || snapshot.frameId !== article.frameId}
            // Scripts only for the app's hash-pinned link forwarder; no same-origin
            // access, popups, forms or top navigation (see readerLayout).
            sandbox="allow-scripts"
            referrerPolicy="no-referrer"
            className="article-preview-frame"
            srcDoc={isolatedReaderDocument(snapshot.html, textSize, readingWidth, theme,
              { images: imagesAllowed && snapshot.source === "online", links: true })}
          />
        ))}
      </div>
      {!article && hits.length > 0 && (
        <div className="online-results">
          <h2>Search results</h2>
          <ul>
            {hits.map(([pageId, title]) => (
              <li key={pageId}>
                <button type="button" disabled={busy || (!hitsFromPack && (offline || !connected || !navigator.onLine))} onClick={() => void openPage(pageId)}>
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

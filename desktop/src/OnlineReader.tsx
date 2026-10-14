import { useEffect, useRef, useState, type FormEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { RequestEpoch } from "./requestEpoch";

type SearchHit = [pageId: number, title: string];
type ArticleResponse = [pageId: number, revisionId: number, title: string, html: string];

interface ArticlePreview {
  pageId: number;
  revisionId: number;
  title: string;
  html: string;
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

export default function OnlineReader() {
  const [query, setQuery] = useState("");
  const [project, setProject] = useState("enwiki");
  const [offline, setOffline] = useState(false);
  const [connected, setConnected] = useState(() => navigator.onLine);
  const requestEpoch = useRef(new RequestEpoch());
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [article, setArticle] = useState<ArticlePreview | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    const disconnected = () => {
      requestEpoch.current.invalidate();
      setConnected(false);
      setBusy(false);
      setHits([]);
      setArticle(null);
      setNotice("Connection lost. This preview has no installed offline articles.");
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
    if (!query.trim() || busy) return;
    const epoch = requestEpoch.current.begin();
    setBusy(true);
    setNotice(null);
    setArticle(null);
    try {
      const result = await invoke<SearchHit[]>("search_wikipedia", { query: query.trim(), project, offline: offline || !connected || !navigator.onLine });
      if (!requestEpoch.current.matches(epoch)) return;
      setHits(result);
      if (!result.length) setNotice("No Wikipedia articles matched that search.");
    } catch (error) {
      if (requestEpoch.current.matches(epoch)) {
        setHits([]);
        setNotice(`Search unavailable: ${String(error)}`);
      }
    } finally {
      if (requestEpoch.current.matches(epoch)) setBusy(false);
    }
  }

  async function openTitle(title: string) {
    if (busy) return;
    const epoch = requestEpoch.current.begin();
    setBusy(true);
    setNotice(null);
    setArticle(null);
    try {
      const [pageId, revisionId, canonicalTitle, html] = await invoke<ArticleResponse>(
        "open_wikipedia_title",
        { title, project, offline: offline || !connected || !navigator.onLine },
      );
      if (!requestEpoch.current.matches(epoch)) return;
      setArticle({ pageId, revisionId, title: canonicalTitle, html });
    } catch (error) {
      if (requestEpoch.current.matches(epoch)) setNotice(`Article unavailable: ${String(error)}`);
    } finally {
      if (requestEpoch.current.matches(epoch)) setBusy(false);
    }
  }

  return (
    <section aria-label="Online Wikipedia reader">
      <form className="online-search" onSubmit={search}>
        <label htmlFor="online-wikipedia-project">Wikipedia edition</label>
        <select id="online-wikipedia-project" value={project} disabled={busy} onChange={(event) => {
          setProject(event.target.value);
          setHits([]);
          setArticle(null);
          setNotice(null);
        }}>
          <option value="enwiki">English</option>
          <option value="dewiki">Deutsch</option>
          <option value="frwiki">Français</option>
          <option value="eswiki">Español</option>
          <option value="jawiki">日本語</option>
        </select>
        <label htmlFor="online-wikipedia-query">Search articles</label>
        <div>
          <input
            id="online-wikipedia-query"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            maxLength={512}
            placeholder="Article or topic"
            disabled={busy}
          />
          <button type="submit" disabled={busy || !query.trim()}>Search</button>
        </div>
      </form>
      <label><input type="checkbox" checked={offline} disabled={busy} onChange={(event) => {
        const nowOffline = event.target.checked;
        setOffline(nowOffline);
        if (nowOffline) {
          // Never present previously fetched online HTML or search as installed content.
          setArticle(null);
          setHits([]);
          setNotice("Offline mode enabled. No article is installed in this preview.");
        } else {
          setNotice(null);
        }
      }} /> Work offline (no network requests)</label>
      <p className="preview-warning">
        {offline || !connected ? "Offline mode: articles require an installed pack, which is not available in this preview. " : ""}
        Online text preview only. Images, equations, and other required media are not saved;
        this page is not offline-ready. Links inside the isolated preview are disabled.
      </p>
      {notice && <p role="status">{notice}</p>}
      {busy && <p role="status">Loading Wikipedia…</p>}
      {article ? (
        <div className="online-article">
          <button type="button" onClick={() => setArticle(null)}>← Back to search results</button>
          <h2>{article.title}</h2>
          <p>Online revision {article.revisionId} · page ID {article.pageId}</p>
          <iframe
            title={`Sanitized Wikipedia article: ${article.title}`}
            sandbox=""
            referrerPolicy="no-referrer"
            className="article-preview-frame"
            srcDoc={isolatedDocument(article.html)}
          />
        </div>
      ) : hits.length > 0 ? (
        <div className="online-results">
          <h2>Search results</h2>
          <ul>
            {hits.map(([pageId, title]) => (
              <li key={pageId}>
                <button type="button" disabled={busy} onClick={() => void openTitle(title)}>
                  {title}
                </button>
              </li>
            ))}
          </ul>
        </div>
      ) : null}
    </section>
  );
}

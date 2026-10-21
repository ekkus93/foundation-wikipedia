import { useEffect, useLayoutEffect, useRef, useState } from "react";
import OnlineReader, { type ReaderArticleContext } from "./OnlineReader";
import { invoke } from "@tauri-apps/api/core";
import { parseReaderBookmarks, type ReaderBookmark } from "./readerResponse";
import { MAX_READER_TABS, addReaderTab, closeReaderTab, initialReaderTabs, selectReaderTab, titleReaderTab } from "./readerTabs";
import { READER_ACTIONS, type ReaderAction } from "./readerActions";
import { SETTINGS_SECTIONS, type SettingsSection } from "./settingsSections";
import { THEMES, isTheme, type Theme } from "./theme";

export default function App() {
  const [expanded, setExpanded] = useState(false);
  const [readerTabs, setReaderTabs] = useState(initialReaderTabs);
  const [message, setMessage] = useState<string | null>(null);
  const [savedBookmarks, setSavedBookmarks] = useState<ReaderBookmark[]>([]);
  const [savedBusy, setSavedBusy] = useState(false);
  const [savedError, setSavedError] = useState<string | null>(null);
  const [destination, setDestination] = useState<"article" | "settings">("article");
  const [settingsSection, setSettingsSection] = useState<SettingsSection | null>(null);
  const [theme, setTheme] = useState<Theme>(() => {
    try {
      const saved = window.localStorage.getItem("foundation-wikipedia.theme");
      return isTheme(saved) ? saved : "system";
    } catch {
      return "system";
    }
  });
  const tabScrollRef = useRef(new Map<number, number>());
  const tabArticleRef = useRef(new Map<number, ReaderArticleContext>());
  const bookmarkInFlightRef = useRef(false);
  const returnFocusRef = useRef(false);
  const settingsBackRef = useRef<HTMLButtonElement>(null);
  const hubRef = useRef<HTMLDivElement>(null);
  const mainActionRef = useRef<HTMLButtonElement>(null);
  const firstActionRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!expanded) return;

    // Put keyboard users on the nearest action when opening the speed dial.
    firstActionRef.current?.focus();

    function onEscape(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setExpanded(false);
        mainActionRef.current?.focus();
      }
    }
    function onOutsidePointer(event: PointerEvent) {
      if (!hubRef.current?.contains(event.target as Node)) {
        setExpanded(false);
      }
    }

    document.addEventListener("keydown", onEscape);
    document.addEventListener("pointerdown", onOutsidePointer);
    return () => {
      document.removeEventListener("keydown", onEscape);
      document.removeEventListener("pointerdown", onOutsidePointer);
    };
  }, [expanded]);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    try {
      window.localStorage.setItem("foundation-wikipedia.theme", theme);
    } catch {
      // The preference remains usable for this session when storage is denied.
    }
  }, [theme]);

  // Restore the outer reading position after switching tabs or returning from Settings.
  // The sandboxed iframe has its own scroll state and is deliberately not given
  // allow-same-origin merely to inspect its internal scroll offset.
  useLayoutEffect(() => {
    if (destination === "article") {
      window.scrollTo(0, tabScrollRef.current.get(readerTabs.activeId) ?? 0);
    }
  }, [destination, readerTabs.activeId]);

  useEffect(() => {
    if (destination === "settings") settingsBackRef.current?.focus();
    if (destination === "article" && returnFocusRef.current) {
      mainActionRef.current?.focus();
      returnFocusRef.current = false;
    }
  }, [destination, settingsSection]);


  useEffect(() => {
    if (destination !== "settings" || settingsSection !== "History & Saved Content") return;
    let active = true;
    setSavedBusy(true);
    setSavedError(null);
    void invoke<unknown>("list_reader_bookmarks")
      .then(parseReaderBookmarks)
      .then((bookmarks) => { if (active) setSavedBookmarks(bookmarks); })
      .catch(() => { if (active) setSavedError("Saved bookmark list is unavailable."); })
      .finally(() => { if (active) setSavedBusy(false); });
    return () => { active = false; };
  }, [destination, settingsSection]);

  async function removeSavedBookmark(project: string, pageId: number) {
    if (savedBusy) return;
    setSavedBusy(true);
    setSavedError(null);
    try {
      await invoke<boolean>("remove_reader_bookmark", { project, pageId });
      setSavedBookmarks((rows) => rows.filter(([p, id]) => p !== project || id !== pageId));
    } catch {
      setSavedError("Unable to remove the saved bookmark.");
    } finally {
      setSavedBusy(false);
    }
  }

  function rememberTabScroll() {
    tabScrollRef.current.set(readerTabs.activeId, window.scrollY);
  }

  function selectTab(id: number) {
    rememberTabScroll();
    setReaderTabs((state) => selectReaderTab(state, id));
  }

  function addTab() {
    rememberTabScroll();
    setReaderTabs(addReaderTab);
  }

  function closeTab(id: number) {
    rememberTabScroll();
    tabScrollRef.current.delete(id);
    tabArticleRef.current.delete(id);
    setReaderTabs((state) => closeReaderTab(state, id));
  }

  function leaveSettings() {
    if (settingsSection !== null) {
      setSettingsSection(null);
    } else {
      returnFocusRef.current = true;
      setDestination("article");
    }
  }

  function activate(action: ReaderAction) {
    setExpanded(false);
    mainActionRef.current?.focus();
    if (action === "Settings") {
      rememberTabScroll();
      setSettingsSection(null);
      setDestination("settings");
      setMessage(null);
    } else if (action === "Bookmark") {
      const current = tabArticleRef.current.get(readerTabs.activeId);
      if (!current) {
        setMessage("Open a Wikipedia article before bookmarking it.");
      } else if (!bookmarkInFlightRef.current) {
        bookmarkInFlightRef.current = true;
        void invoke<boolean>("toggle_reader_bookmark", { project: current.project, pageId: current.pageId, revisionId: current.revisionId })
          .then((saved) => setMessage(saved
            ? "Page bookmark saved. Article text and images were not downloaded for offline use."
            : "Page bookmark removed."))
          .catch(() => setMessage("Bookmark storage unavailable; no offline content was changed."))
          .finally(() => { bookmarkInFlightRef.current = false; });
      }
    } else {
      setMessage(`${action} is not implemented yet. This screen is a development shell.`);
    }
  }

  return (
    <div className="shell">
      <header className="topbar">
        <div className="brand"><span aria-hidden="true" className="brandmark">W</span><span>
          <strong>Wikipedia</strong><small>Foundation reader · preview</small>
        </span></div>
        <span className="search-hint">Search Wikipedia in the reader below</span>
        <span className="status">Development preview</span>
      </header>
      <div className="columns" hidden={destination !== "article"}>
        <nav className="contents" aria-label="Contents">
          <strong>Contents</strong>
          <a href="#overview">Overview</a>
          <a href="#roadmap">Development</a>
        </nav>
        <main>
          <p className="eyebrow">FOUNDATION WIKIPEDIA</p>
          <h1 id="overview">Wikipedia reader preview</h1>
          <nav className="reader-tabs" aria-label="Open article tabs">
            {readerTabs.tabs.map((tab) => (
              <div className="reader-tab" key={tab.id}>
                <button type="button" aria-current={readerTabs.activeId === tab.id ? "page" : undefined}
                  onClick={() => selectTab(tab.id)}>
                  {tab.title}
                </button>
                <button type="button" aria-label={`Close tab ${tab.id}: ${tab.title}`}
                  disabled={readerTabs.tabs.length === 1}
                  onClick={() => closeTab(tab.id)}>×</button>
              </div>
            ))}
            <button type="button" disabled={readerTabs.tabs.length >= MAX_READER_TABS}
              onClick={addTab}>+ New tab</button>
          </nav>
          {readerTabs.tabs.map((tab) => (
            <section key={tab.id} aria-label={`Article tab ${tab.id}`}
              hidden={readerTabs.activeId !== tab.id}>
              <OnlineReader onArticleChange={(title, context) => {
                if (context) tabArticleRef.current.set(tab.id, context);
                else tabArticleRef.current.delete(tab.id);
                setReaderTabs((state) => titleReaderTab(state, tab.id, title));
              }} />
            </section>
          ))}
          <h2 id="roadmap">Under development</h2>
          <p>Offline media, installed packs, grounded AI, bookmarks, and complete article navigation are still in development.</p>
        </main>
      </div>
      {destination === "settings" && <main className="settings-view" aria-label="Settings">
        <button className="settings-back" ref={settingsBackRef} onClick={leaveSettings}>
          ← {settingsSection === null ? "Return to article" : "All settings"}
        </button>
        <h1>{settingsSection ?? "Settings"}</h1>
        {settingsSection === null ? <nav className="settings-sections" aria-label="Settings sections">
          {SETTINGS_SECTIONS.map((section) => <button key={section} onClick={() => setSettingsSection(section)}>{section} →</button>)}
        </nav> : settingsSection === "Appearance & Reading" ? <fieldset className="theme-choice">
          <legend>Color theme</legend>
          {THEMES.map((choice) => <label key={choice}>
            <input type="radio" name="color-theme" value={choice} checked={theme === choice} onChange={() => setTheme(choice)} />
            {choice === "system" ? "Use system setting" : choice === "light" ? "Light" : "Dark"}
          </label>)}
        </fieldset> : settingsSection === "History & Saved Content" ? <section aria-label="Saved page bookmarks">
          <p>Saved Wikipedia page identities only. Bookmarks do not install article text or images for offline use.</p>
          {savedBusy && <p role="status">Loading saved bookmarks…</p>}
          {savedError && <p role="alert">{savedError}</p>}
          {!savedBusy && !savedError && savedBookmarks.length === 0 && <p>No saved pages.</p>}
          <ul>{savedBookmarks.map(([project, pageId, revisionId]) => <li key={project + ":" + pageId}>
            {project} · page {pageId} · saved revision {revisionId}{" "}
            <button type="button" disabled={savedBusy}
              aria-label={`Remove bookmark for ${project} page ${pageId}`}
              onClick={() => void removeSavedBookmark(project, pageId)}>Remove</button>
          </li>)}</ul>
          <p>Opening a bookmark at the latest revision and restoring its reading position are not available yet.</p>
        </section> : <p>Settings for {settingsSection} are not available in this development preview.</p>}
      </main>}
      {destination === "article" && message && <div className="notice" role="status">
        {message}<button aria-label="Dismiss notice" onClick={() => setMessage(null)}>×</button>
      </div>}
      {destination === "article" && <div className="hub" ref={hubRef}>
        {expanded && <div className="speed-dial" role="group" aria-label="Reader actions" id="reader-action-menu">
          {READER_ACTIONS.map((action, index) => <button key={action} ref={index === 0 ? firstActionRef : undefined} onClick={() => activate(action)}>{action}</button>)}
        </div>}
        <button className="fab" ref={mainActionRef}
          aria-label={expanded ? "Close reader actions" : "Open reader actions"}
          aria-expanded={expanded}
          aria-controls={expanded ? "reader-action-menu" : undefined}
          onClick={() => setExpanded(!expanded)}>{expanded ? "×" : "✦"}</button>
      </div>}
    </div>
  );
}

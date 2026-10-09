import { useEffect, useRef, useState } from "react";
import { READER_ACTIONS, type ReaderAction } from "./readerActions";

export default function App() {
  const [expanded, setExpanded] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const hubRef = useRef<HTMLDivElement>(null);
  const mainActionRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!expanded) return;

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

  function activate(action: ReaderAction) {
    setExpanded(false);
    mainActionRef.current?.focus();
    setMessage(`${action} is not implemented yet. This screen is a development shell.`);
  }

  return (
    <div className="shell">
      <header className="topbar">
        <div className="brand"><span aria-hidden="true" className="brandmark">W</span><span>
          <strong>Wikipedia</strong><small>Foundation reader · preview</small>
        </span></div>
        <input className="search" aria-label="Wikipedia search not yet available" disabled placeholder="Search Wikipedia (coming soon)" />
        <span className="status">Development preview</span>
      </header>
      <div className="columns">
        <nav className="contents" aria-label="Contents">
          <strong>Contents</strong>
          <a href="#overview">Overview</a>
          <a href="#roadmap">Development</a>
        </nav>
        <main>
          <p className="eyebrow">FOUNDATION WIKIPEDIA</p>
          <h1>Welcome to Foundation Wikipedia</h1>
          <p id="overview" className="lead">A Wikipedia-first reader with optional AI assistance and offline topic packs.</p>
          <h2 id="roadmap">Under development</h2>
          <p>This is a placeholder article surface. Wikipedia fetch/search, offline packs and grounded AI are not implemented yet. Article rendering will preserve familiar Wikipedia layouts.</p>
          <p>Project details are in the <a href="https://github.com/ekkus93/foundation-wikipedia/blob/master/docs/WIKIPEDIA_AI_READER_SPEC.md" target="_blank" rel="noreferrer">technical specification</a>.</p>
        </main>
      </div>
      {message && <div className="notice" role="status">
        {message}<button aria-label="Dismiss notice" onClick={() => setMessage(null)}>×</button>
      </div>}
      <div className="hub" ref={hubRef}>
        {expanded && <div className="speed-dial" role="group" aria-label="Reader actions" id="reader-action-menu">
          {READER_ACTIONS.map((action) => <button key={action} onClick={() => activate(action)}>{action}</button>)}
        </div>}
        <button className="fab" ref={mainActionRef}
          aria-label={expanded ? "Close reader actions" : "Open reader actions"}
          aria-expanded={expanded}
          aria-controls="reader-action-menu"
          onClick={() => setExpanded(!expanded)}>{expanded ? "×" : "✦"}</button>
      </div>
    </div>
  );
}

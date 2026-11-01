import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import { clampReaderChatPosition, type ReaderChatPosition } from "./readerChatWindow";
import { invoke } from "@tauri-apps/api/core";
import type { ReaderArticleContext } from "./OnlineReader";
import { parseReaderChatExchanges, parseReaderChatThreads, type ReaderChatExchange } from "./readerChatHistory";

/** Read-only historical transcript: an online HTML preview is not canonical RAG
 * evidence, so this component never generates, transmits or certifies citations. */
export default function ReaderChatHistory({ article, onClose }: {
  article: ReaderArticleContext;
  onClose: () => void;
}) {
  const [threads, setThreads] = useState<number[]>([]);
  const [selectedThread, setSelectedThread] = useState<number | null>(null);
  const [exchanges, setExchanges] = useState<ReaderChatExchange[]>([]);
  const [loadingThreads, setLoadingThreads] = useState(true);
  const [loadingExchanges, setLoadingExchanges] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<number | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [deleteNotice, setDeleteNotice] = useState<string | null>(null);
  const [minimized, setMinimized] = useState(false);
  const [position, setPosition] = useState<ReaderChatPosition | null>(null);
  const panelRef = useRef<HTMLElement>(null);
  const dragRef = useRef<{
    pointerId: number; startX: number; startY: number; left: number; top: number;
  } | null>(null);

  // Constrain both native CSS resizing and viewport resizing. Moving this
  // nonmodal panel never changes the article or conversation identity.
  useLayoutEffect(() => {
    const panel = panelRef.current;
    if (!panel) return;
    function constrain() {
      const rect = panel!.getBoundingClientRect();
      setPosition((previous) => {
        const next = clampReaderChatPosition(
          previous?.left ?? rect.left, previous?.top ?? rect.top,
          rect.width, rect.height, window.innerWidth, window.innerHeight,
        );
        return previous?.left === next.left && previous?.top === next.top ? previous : next;
      });
    }
    constrain();
    window.addEventListener("resize", constrain);
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(constrain);
    observer?.observe(panel);
    return () => { window.removeEventListener("resize", constrain); observer?.disconnect(); };
  }, []);

  function moveTo(left: number, top: number) {
    const panel = panelRef.current;
    if (!panel) return;
    const rect = panel.getBoundingClientRect();
    setPosition(clampReaderChatPosition(
      left, top, rect.width, rect.height, window.innerWidth, window.innerHeight,
    ));
  }

  function startDrag(event: PointerEvent<HTMLDivElement>) {
    if (event.button !== 0 || !panelRef.current) return;
    const rect = panelRef.current.getBoundingClientRect();
    dragRef.current = {
      pointerId: event.pointerId, startX: event.clientX, startY: event.clientY,
      left: rect.left, top: rect.top,
    };
    event.currentTarget.setPointerCapture(event.pointerId);
    event.preventDefault();
  }

  function drag(event: PointerEvent<HTMLDivElement>) {
    const active = dragRef.current;
    if (active?.pointerId !== event.pointerId) return;
    moveTo(active.left + event.clientX - active.startX, active.top + event.clientY - active.startY);
  }

  function stopDrag(event: PointerEvent<HTMLDivElement>) {
    if (dragRef.current?.pointerId !== event.pointerId) return;
    dragRef.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  }

  function moveWithKeyboard(event: KeyboardEvent<HTMLDivElement>) {
    const directions: Record<string, [number, number]> = {
      ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1],
    };
    const direction = directions[event.key];
    if (!direction || !panelRef.current) return;
    event.preventDefault();
    const rect = panelRef.current.getBoundingClientRect();
    const step = event.shiftKey ? 48 : 16;
    moveTo(rect.left + direction[0] * step, rect.top + direction[1] * step);
  }

  useEffect(() => {
    let active = true;
    setThreads([]);
    setSelectedThread(null);
    setExchanges([]);
    setError(null);
    setConfirmDelete(null);
    setDeleteNotice(null);
    setLoadingThreads(true);
    void invoke<unknown>("list_reader_chat_threads", {
      project: article.project, pageId: article.pageId, revisionId: article.revisionId,
    }).then(parseReaderChatThreads).then((ids) => {
      if (!active) return;
      setThreads(ids);
      setSelectedThread(ids[0] ?? null);
    }).catch(() => {
      if (active) setError("Saved conversations could not be loaded.");
    }).finally(() => { if (active) setLoadingThreads(false); });
    return () => { active = false; };
  }, [article.project, article.pageId, article.revisionId]);

  useEffect(() => {
    if (selectedThread === null) {
      setExchanges([]);
      return;
    }
    let active = true;
    setExchanges([]);
    setError(null);
    setLoadingExchanges(true);
    void invoke<unknown>("read_reader_chat_exchanges", {
      project: article.project, pageId: article.pageId, revisionId: article.revisionId,
      threadId: selectedThread,
    }).then(parseReaderChatExchanges).then((rows) => {
      if (active) setExchanges(rows);
    }).catch(() => {
      if (active) setError("This saved conversation could not be read.");
    }).finally(() => { if (active) setLoadingExchanges(false); });
    return () => { active = false; };
  }, [article.project, article.pageId, article.revisionId, selectedThread]);

  async function deleteSelectedThread() {
    const id = selectedThread;
    if (id === null || confirmDelete !== id || deleting || !threads.includes(id)) return;
    setDeleting(true);
    setError(null);
    setDeleteNotice(null);
    try {
      const removed = await invoke<boolean>("delete_reader_chat_thread", {
        project: article.project, pageId: article.pageId, revisionId: article.revisionId,
        threadId: id,
      });
      if (!removed) throw new Error("Thread was not removed");
      const remaining = threads.filter((thread) => thread !== id);
      setThreads(remaining);
      setSelectedThread(remaining[0] ?? null);
      setExchanges([]);
      setDeleteNotice("Conversation deleted from private local storage.");
    } catch {
      setError("Conversation could not be deleted; refresh the history before trying again.");
    } finally {
      setConfirmDelete(null);
      setDeleting(false);
    }
  }

  return <aside ref={panelRef} className="reader-chat-history" role="dialog"
    aria-label="Saved article conversations" data-minimized={minimized}
    style={position ? { left: position.left, top: position.top, right: "auto" } : undefined}>
    <header>
      <div className="reader-chat-drag-handle" role="button" tabIndex={0}
        aria-label="Move saved conversations window; use arrow keys"
        onPointerDown={startDrag} onPointerMove={drag}
        onPointerUp={stopDrag} onPointerCancel={stopDrag} onKeyDown={moveWithKeyboard}>
        <strong>Saved article conversations</strong>
      </div>
      <div className="reader-chat-header-actions">
        <button type="button" onClick={() => setMinimized((value) => !value)}
          aria-expanded={!minimized} aria-label={minimized ? "Restore saved conversations" : "Minimize saved conversations"}>
          {minimized ? "□" : "−"}
        </button>
        <button type="button" onClick={onClose} aria-label="Close saved conversations">×</button>
      </div>
    </header>
    <div className="reader-chat-body" hidden={minimized}>
    <p>Edition {article.project} · page {article.pageId} · revision {article.revisionId}</p>
    <p className="reader-chat-disclaimer">Read-only history. Saved answers are plain text and their citation claims are not revalidated. The online preview cannot supply canonical article evidence for new AI questions.</p>
    {loadingThreads && <p role="status">Loading saved conversations…</p>}
    {error && <p role="alert">{error}</p>}
    {deleteNotice && <p role="status">{deleteNotice}</p>}
    {!loadingThreads && !error && threads.length === 0 && <p>No saved conversations for this exact revision.</p>}
    {threads.length > 0 && <label>Conversation
      <select value={selectedThread ?? ""} disabled={deleting} onChange={(event) => {
        setConfirmDelete(null);
        setSelectedThread(Number(event.target.value));
      }}>
        {threads.map((id) => <option key={id} value={id}>Conversation {id}</option>)}
      </select>
    </label>}
    {selectedThread !== null && (confirmDelete !== selectedThread ? (
      <button type="button" disabled={deleting || loadingExchanges}
        onClick={() => setConfirmDelete(selectedThread)}>Delete this conversation…</button>
    ) : <div role="group" aria-label="Confirm conversation deletion">
      <p>Permanently delete conversation {selectedThread} and all its saved messages? Other articles, revisions, bookmarks and packs are unchanged.</p>
      <button type="button" disabled={deleting} onClick={() => void deleteSelectedThread()}>Yes, delete conversation</button>
      <button type="button" disabled={deleting} onClick={() => setConfirmDelete(null)}>Cancel</button>
    </div>)}
    {loadingExchanges && <p role="status">Loading conversation…</p>}
    {!loadingExchanges && !error && selectedThread !== null && exchanges.length === 0 && <p>No complete exchanges in this conversation.</p>}
    <ol className="reader-chat-exchanges">{exchanges.map(([questionId, answerId, question, answer]) =>
      <li key={questionId + ":" + answerId}>
        <p><strong>Question</strong></p><p className="reader-chat-text">{question}</p>
        <p><strong>Saved answer (unverified)</strong></p><p className="reader-chat-text">{answer}</p>
      </li>)}</ol>
    </div>
  </aside>;
}

import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  DRAFT_LABEL, numberCitations, parseChatAnswer, parseChatPreview, parseChatProgress,
  type ChatExchange, type ChatPassage, type ChatPreview,
} from "./packChat";
import type { ReaderArticleContext } from "./OnlineReader";

/**
 * UX-002 grounded chat about the current installed-pack article. The user
 * reviews exactly what will be sent (destination, model, passages) before
 * anything leaves the app; the answer's citations open the cited passage of
 * the same revision in the reader.
 */
export default function ArticleChat({ article, exchanges, onExchange, selection, onClearSelection, onCitation }: {
  article: ReaderArticleContext;
  exchanges: ChatExchange[];
  onExchange: (exchange: ChatExchange) => void;
  selection: string | null;
  onClearSelection: () => void;
  onCitation: (passage: ChatPassage) => void;
}) {
  const [question, setQuestion] = useState("");
  const [preview, setPreview] = useState<ChatPreview | null>(null);
  const [sending, setSending] = useState<number | null>(null);
  const [draft, setDraft] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const sendingRef = useRef<number | null>(null);
  sendingRef.current = sending;

  useEffect(() => {
    if (selection && !question.trim()) setQuestion("What does this mean?");
  }, [selection]);

  useEffect(() => {
    let stop: (() => void) | null = null;
    let cancelled = false;
    void listen<unknown>("pack-chat-progress", (event) => {
      const progress = parseChatProgress(event.payload);
      if (progress && progress.id === sendingRef.current) setDraft(progress.draft);
    }).then((unlisten) => {
      if (cancelled) unlisten();
      else stop = unlisten;
    });
    return () => {
      cancelled = true;
      stop?.();
    };
  }, []);

  if (article.source !== "pack") {
    return <section className="article-chat" aria-label="Ask about this article">
      <p>Chat answers questions from an article's own text, which needs the article from an installed pack.
        Turn on <em>Work offline</em> and open the article from the active pack.</p>
    </section>;
  }

  async function review() {
    setBusy(true);
    setMessage(null);
    try {
      setPreview(parseChatPreview(await invoke<unknown>("prepare_pack_chat", {
        project: article.project, pageId: article.pageId, question, selection,
      })));
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  }

  async function send(pending: ChatPreview) {
    const asked = question.trim();
    setPreview(null);
    setSending(pending.id);
    setDraft(null);
    setMessage(null);
    try {
      const answer = parseChatAnswer(await invoke<unknown>("send_pack_chat", { id: pending.id }));
      onExchange({ question: asked, answer });
      setQuestion("");
      if (selection) onClearSelection();
    } catch (error) {
      setMessage(String(error));
    } finally {
      setSending(null);
      setDraft(null);
    }
  }

  return <section className="article-chat" aria-label="Ask about this article">
    <ol className="article-chat-exchanges" aria-label="Conversation">
      {exchanges.map((exchange, index) => {
        if (exchange.answer.kind !== "supported") {
          return <li key={index}><p className="article-chat-question">{exchange.question}</p>
            <p>The article passages do not answer this question.</p></li>;
        }
        const { segments, citations } = numberCitations(exchange.answer.parts);
        return <li key={index}>
          <p className="article-chat-question">{exchange.question}</p>
          <p>{segments.map((segment, part) => segment.kind === "text" ? <span key={part}>{segment.text}</span>
            : <sup key={part}>[{segment.number}]</sup>)}</p>
          <div className="article-chat-citations">
            {citations.map(({ number, passage }) => <button type="button" key={number}
              aria-label={`Show cited passage ${number}: ${passage.excerpt.slice(0, 80)}`}
              onClick={() => onCitation(passage)}>[{number}] {passage.headingPath.join(" › ") || "Introduction"}</button>)}
          </div>
        </li>;
      })}
    </ol>
    {sending !== null && <div className="article-chat-draft" aria-live="polite">
      <p className="article-chat-draft-label">{DRAFT_LABEL}</p>
      <p>{draft || "Waiting for the model…"}</p>
      <button type="button" onClick={() => void invoke("cancel_pack_chat", { id: sending })}>Stop</button>
    </div>}
    {message && <p role="status">{message}</p>}
    {selection && <p className="article-chat-selection">Asking about: “{selection.slice(0, 200)}{selection.length > 200 ? "…" : ""}”
      <button type="button" onClick={onClearSelection} aria-label="Stop asking about the selection">×</button></p>}
    {preview ? <div className="article-chat-review" role="group" aria-label="Request review">
      <p><strong>This will be sent to {preview.destination}, on {preview.locality === "this device" ? "this device" : preview.locality === "local network" ? "your local network" : "the internet"} (model {preview.model}, profile {preview.profile}):</strong></p>
      <p>Question: “{question.trim()}”</p>
      {preview.selectedPassage && <p>Your selection is in the passage from {preview.selectedPassage.headingPath.join(" › ") || "Introduction"}, which is always included.</p>}
      <p>{preview.passages.length} passage{preview.passages.length === 1 ? "" : "s"} from “{preview.articleTitle}”, revision {preview.revisionId}:</p>
      <ul>{preview.passages.map((passage) => <li key={passage.blockId}>
        {passage.headingPath.join(" › ") || "Introduction"}: {passage.excerpt.slice(0, 280)}</li>)}</ul>
      <button type="button" onClick={() => void send(preview)}>Send</button>
      <button type="button" onClick={() => { void invoke("cancel_pack_chat", { id: preview.id }); setPreview(null); setMessage("Nothing was sent."); }}>Cancel</button>
    </div> : sending === null && <form onSubmit={(event) => { event.preventDefault(); void review(); }}>
      <label htmlFor="article-chat-question">Question</label>
      <textarea id="article-chat-question" value={question} maxLength={2000} rows={3}
        onChange={(event) => setQuestion(event.target.value)} />
      <button type="submit" disabled={busy || !question.trim()}>Review request</button>
      <p className="article-chat-note">Nothing is sent until you review the request and choose Send.</p>
    </form>}
  </section>;
}

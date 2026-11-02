import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { activeAiLocalChatProfile, aiLocalityDisclosure, parseAiSavedLocalProfiles, shouldPollAiProfiles, type AiSavedLocalProfiles } from "./aiLocalModels";

/** Display only the persisted local profile destination. This component
 * never discovers models, sends article evidence, or grants inference consent.
 */
export default function ReaderChatProviderStatus() {
  const [profiles, setProfiles] = useState<AiSavedLocalProfiles | null>(null);
  const [readError, setReadError] = useState(false);

  useEffect(() => {
    let mounted = true;
    let generation = 0;
    let inFlight = false;
    let pending = false;
    function refresh() {
      if (!mounted) return;
      generation += 1;
      if (inFlight) {
        pending = true;
        return;
      }
      inFlight = true;
      const request = generation;
      void invoke<unknown>("read_local_ai_profiles").then(parseAiSavedLocalProfiles).then((saved) => {
        if (!mounted || request !== generation) return;
        setProfiles(saved);
        setReadError(false);
      }).catch(() => {
        if (!mounted || request !== generation) return;
        setProfiles(null);
        setReadError(true);
      }).finally(() => {
        inFlight = false;
        if (mounted && pending) {
          pending = false;
          refresh();
        }
      });
    }
    const onVisibility = () => {
      if (shouldPollAiProfiles(document.visibilityState)) refresh();
    };
    refresh();
    window.addEventListener("focus", refresh);
    document.addEventListener("visibilitychange", onVisibility);
    const poll = window.setInterval(onVisibility, 5000);
    let unlisten: (() => void) | undefined;
    void listen("ai-local-profiles-changed", refresh).then((dispose) => {
      if (mounted) unlisten = dispose;
      else dispose();
    }).catch(() => {});
    return () => {
      mounted = false;
      generation += 1;
      window.clearInterval(poll);
      unlisten?.();
      window.removeEventListener("focus", refresh);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, []);

  const active = profiles && activeAiLocalChatProfile(profiles);
  return <section aria-label="Saved AI model destination">
    {readError ? <p role="alert">Cannot read saved AI model selection. No model is contacted.</p>
      : !profiles ? <p role="status">Reading saved AI model selection…</p>
      : active ? <>
        <p>Saved local profile: {active.name} · model {active.model} · {active.protocol}</p>
        <p>Destination: {active.endpoint}</p>
        <p>{aiLocalityDisclosure(active.locality)}</p>
      </> : <p>No local AI model is currently active. Select one in AI Assistant Settings.</p>}
    <p className="reader-chat-disclaimer">This is a saved model hint, not a verified connection or permission to send passages. Chat generation is not yet available.</p>
  </section>;
}

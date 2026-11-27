import { useEffect, useRef, useState } from "react";
import { AV_START_TIMEOUT_MS, formatClock, nextCandidate, type AvStreamInfo } from "./avPlayer";

/**
 * MOBPACK-003: plays an article's audio or video inside the app, as the
 * Android player does. Each address is tried in turn; one that fails, or has
 * not started within 30 s, gives way to the next. Only addresses the
 * `av_stream` command produced (all on upload.wikimedia.org, which the CSP
 * allows for media and nothing else) reach the element.
 */
export function AvPlayer({ info, onClose, onOpenInBrowser }: {
  info: AvStreamInfo;
  onClose: () => void;
  onOpenInBrowser: () => void;
}) {
  const [index, setIndex] = useState<number | null>(0);
  const [started, setStarted] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [position, setPosition] = useState(0);
  const [duration, setDuration] = useState(0);
  const media = useRef<HTMLVideoElement & HTMLAudioElement>(null);
  const count = info.candidates.length;

  function giveWay() {
    setIndex((current) => (current === null ? null : nextCandidate(current, count)));
    setStarted(false);
    setPlaying(false);
    setPosition(0);
    setDuration(0);
  }

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  // A candidate that has not started within 30 s gives way to the next.
  useEffect(() => {
    if (index === null || started) return;
    const timer = window.setTimeout(() => {
      setIndex((current) => (current === null ? null : nextCandidate(current, count)));
      setPosition(0);
      setDuration(0);
    }, AV_START_TIMEOUT_MS);
    return () => window.clearTimeout(timer);
  }, [index, started, count]);

  function toggle() {
    const element = media.current;
    if (!element) return;
    if (element.paused) void element.play().catch(() => undefined);
    else element.pause();
  }

  const common = {
    ref: media,
    src: index === null ? undefined : info.candidates[index],
    autoPlay: true,
    preload: "auto" as const,
    onLoadedData: () => setStarted(true),
    onPlaying: () => { setStarted(true); setPlaying(true); },
    onPause: () => setPlaying(false),
    onEnded: () => setPlaying(false),
    onError: giveWay,
    onTimeUpdate: () => setPosition(media.current?.currentTime ?? 0),
    onDurationChange: () => {
      const length = media.current?.duration ?? 0;
      setDuration(Number.isFinite(length) ? length : 0);
    },
  };

  return (
    <div className="av-player" role="dialog" aria-modal="true" aria-label={`Playing ${info.title}`}>
      <div className="av-player-body">
        <h2 className="av-player-title">{info.title}</h2>
        {index === null ? (
          <div>
            <p role="status">This could not be played here.</p>
            <button type="button" onClick={onOpenInBrowser}>Open in browser</button>{" "}
            <button type="button" autoFocus onClick={onClose}>Close</button>
          </div>
        ) : (
          <>
            {info.video
              ? <video key={index} {...common} className="av-player-video" />
              : <audio key={index} {...common} />}
            {!started && <p role="status">Loading…</p>}
            <div className="av-player-controls">
              <button type="button" onClick={toggle} disabled={!started}>{playing ? "Pause" : "Play"}</button>
              <input
                type="range"
                aria-label="Position"
                min={0}
                max={duration || 1}
                step={0.1}
                value={Math.min(position, duration || 1)}
                disabled={!started || duration <= 0}
                onChange={(event) => {
                  const to = Number(event.target.value);
                  if (media.current) media.current.currentTime = to;
                  setPosition(to);
                }}
              />
              <span aria-label="Elapsed and total time">{formatClock(position)} / {formatClock(duration)}</span>
              <button type="button" autoFocus onClick={onClose}>Close</button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

/** A verified pack awaiting the user's confirmation. */
interface PackReview {
  path: string;
  snapshotId: string;
  project: string;
  trustLabel: string;
  trustDescription: string;
  articles: number;
  declaredBytes: number;
}

interface InstalledPack {
  snapshotId: string;
  project: string;
  trustLabel: string;
  trustDescription: string;
  articles: number;
  active: boolean;
  damaged: boolean;
}

const text = (value: unknown, max = 512): string => {
  if (typeof value !== "string" || value.length > max) throw new Error("Invalid offline pack data");
  return value;
};
const count = (value: unknown): number => {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) throw new Error("Invalid offline pack data");
  return value;
};
const flag = (value: unknown): boolean => {
  if (typeof value !== "boolean") throw new Error("Invalid offline pack data");
  return value;
};

/** Validate the native review DTO before showing it. */
export function parsePackReview(value: unknown, path: string): PackReview {
  const record = value as Record<string, unknown>;
  if (typeof value !== "object" || value === null) throw new Error("Invalid offline pack data");
  return {
    path,
    snapshotId: text(record.snapshot_id, 256),
    project: text(record.project, 64),
    trustLabel: text(record.trust_label, 64),
    trustDescription: text(record.trust_description),
    articles: count(record.articles),
    declaredBytes: count(record.declared_bytes),
  };
}

/** Validate the native installed-pack list. */
export function parseInstalledPacks(value: unknown): InstalledPack[] {
  if (!Array.isArray(value) || value.length > 1000) throw new Error("Invalid offline pack data");
  return value.map((item) => {
    const record = item as Record<string, unknown>;
    if (typeof item !== "object" || item === null) throw new Error("Invalid offline pack data");
    return {
      snapshotId: text(record.snapshot_id, 256),
      project: text(record.project, 64),
      trustLabel: text(record.trust_label, 64),
      trustDescription: text(record.trust_description),
      articles: count(record.articles),
      active: flag(record.active),
      damaged: flag(record.damaged),
    };
  });
}

function formatBytes(bytes: number): string {
  const mib = bytes / (1024 * 1024);
  return mib >= 1 ? `${mib.toFixed(1)} MB` : `${bytes} bytes`;
}

/** The trust status with its explanation (shared text from Rust). */
function TrustStatus({ label, description }: { label: string; description: string }) {
  return (
    <div className="pack-trust" aria-label={`Trust status: ${label}. ${description}`}>
      <strong>{label}</strong>
      <span>{description}</span>
    </div>
  );
}

/**
 * Settings › Wikipedia & Offline Data: import a custom `.wpack` (verified
 * and reviewed before installing), and activate or remove installed packs.
 * With Work offline on, the reader searches and reads the active pack.
 */
export default function OfflinePacksSettings() {
  const [packs, setPacks] = useState<InstalledPack[] | null>(null);
  const [review, setReview] = useState<PackReview | null>(null);
  const [confirmRemove, setConfirmRemove] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  async function refresh() {
    try {
      setPacks(parseInstalledPacks(await invoke<unknown>("list_offline_packs")));
    } catch (error) {
      setMessage(String(error));
    }
  }

  useEffect(() => {
    void refresh();
  }, []);

  async function run(action: () => Promise<string | null>) {
    setBusy(true);
    setMessage(null);
    try {
      setMessage(await action());
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  }

  const importPack = () => run(async () => {
    const path = await invoke<unknown>("pick_offline_pack");
    if (path === null) return null;
    const chosen = text(path, 4096);
    setMessage("Verifying pack…");
    setReview(parsePackReview(await invoke<unknown>("review_offline_pack", { path: chosen }), chosen));
    return null;
  });

  const install = (pending: PackReview) => run(async () => {
    setReview(null);
    const id = text(await invoke<unknown>("install_offline_pack", { path: pending.path }), 256);
    await refresh();
    return `Installed ${id}. Activate it to read it offline.`;
  });

  const activate = (id: string) => run(async () => {
    await invoke("activate_offline_pack", { snapshotId: id });
    await refresh();
    return `${id} is now the active pack. Turn on Work offline to read it.`;
  });

  const remove = (id: string) => run(async () => {
    setConfirmRemove(null);
    await invoke("remove_offline_pack", { snapshotId: id });
    await refresh();
    return `Removed ${id}.`;
  });

  return (
    <section className="offline-packs" aria-label="Offline packs">
      <h2>Offline packs</h2>
      <p>
        Read Wikipedia without a connection from installed packs. Turn on <em>Work offline</em> in
        the reader to search and read the active pack. Custom packs are checked for integrity
        before installing and are never shown as coming from a verified publisher.
      </p>
      <button type="button" disabled={busy || review !== null} onClick={() => void importPack()}>
        Import a .wpack file
      </button>
      {message && <p role="status">{message}</p>}

      {review && (
        <div className="pack-review" role="dialog" aria-label="Install pack?">
          <h3>Install this pack?</h3>
          <TrustStatus label={review.trustLabel} description={review.trustDescription} />
          <dl>
            <dt>Pack</dt><dd>{review.snapshotId}</dd>
            <dt>Wikipedia edition</dt><dd>{review.project}</dd>
            <dt>Articles</dt><dd>{review.articles}</dd>
            <dt>Size</dt><dd>{formatBytes(review.declaredBytes)}</dd>
          </dl>
          <button type="button" disabled={busy} onClick={() => void install(review)}>Install</button>
          <button type="button" disabled={busy} onClick={() => { setReview(null); setMessage("Import cancelled. Nothing was installed."); }}>Cancel</button>
        </div>
      )}

      <h3>Installed packs</h3>
      {packs === null ? <p>Loading…</p> : packs.length === 0 ? <p>No offline packs are installed.</p> : (
        <ul className="installed-packs">
          {packs.map((pack) => (
            <li key={pack.snapshotId}>
              <strong>{pack.snapshotId}</strong>{pack.active ? " (active)" : ""}
              <TrustStatus label={pack.trustLabel} description={pack.trustDescription} />
              {!pack.damaged && <span>{pack.project} · {pack.articles} articles</span>}
              <div>
                {!pack.active && !pack.damaged && (
                  <button type="button" disabled={busy} onClick={() => void activate(pack.snapshotId)}>Activate</button>
                )}
                {!pack.active && (confirmRemove === pack.snapshotId ? (
                  <>
                    <span>Remove {pack.snapshotId}? </span>
                    <button type="button" disabled={busy} onClick={() => void remove(pack.snapshotId)}>Remove</button>
                    <button type="button" disabled={busy} onClick={() => setConfirmRemove(null)}>Cancel</button>
                  </>
                ) : (
                  <button type="button" disabled={busy} onClick={() => setConfirmRemove(pack.snapshotId)}>Remove…</button>
                ))}
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

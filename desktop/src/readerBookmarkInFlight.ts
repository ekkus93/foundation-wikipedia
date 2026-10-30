/** A privacy reset invalidates pending bookmark IPC requests without waiting for
 * them to finish. A stale finalizer must never release a newer request. */
export class ReaderBookmarkInFlight {
  private nextId = 0;
  private activeId: number | null = null;

  begin(): number | null {
    if (this.activeId !== null) return null;
    const id = ++this.nextId;
    this.activeId = id;
    return id;
  }

  invalidate(): void {
    this.activeId = null;
  }

  finish(id: number): void {
    if (this.activeId === id) this.activeId = null;
  }
}

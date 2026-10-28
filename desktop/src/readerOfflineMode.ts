/** Shared synchronous offline switch for all desktop article tabs.
 * This is a UI request gate, not an OS-level cancellation of HTTP already
 * in progress. Native transport must still enforce offline requests itself.
 */
export class ReaderOfflineMode {
  private offline = false;
  private readonly listeners = new Set<() => void>();

  getSnapshot = (): boolean => this.offline;

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => { this.listeners.delete(listener); };
  };

  setOffline(next: boolean): void {
    if (this.offline === next) return;
    this.offline = next;
    for (const listener of this.listeners) listener();
  }
}

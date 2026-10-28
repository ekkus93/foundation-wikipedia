/** Invalidate stale bookmark IPC completions and block new bookmark actions
 * while the native privacy-reset transaction is in flight. Native generation
 * fencing independently protects queued SQLite writes across the reset. */
export class ReaderPersonalDataSession {
  private revision = 0;
  private resetting = false;

  beginReset(): boolean {
    if (this.resetting) return false;
    this.resetting = true;
    this.revision += 1;
    return true;
  }

  finishReset(): void {
    this.resetting = false;
  }

  canWrite(): boolean {
    return !this.resetting;
  }

  snapshot(): number {
    return this.revision;
  }

  accepts(revision: number): boolean {
    return !this.resetting && this.revision === revision;
  }
}

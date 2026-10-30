/** Serialize durable preference writes so an older IPC cannot overwrite a newer
 * choice. Ignore failures from superseded choices; the newest failure is visible.
 * The UI remains optimistic while persistence is pending. */
export class ReaderLatestPreferenceWriter<T> {
  private latest: symbol | null = null;
  private tail: Promise<void> = Promise.resolve();

  submit(value: T, write: (value: T) => Promise<void>, onLatestFailure: () => void): Promise<void> {
    const request = Symbol("preference write");
    this.latest = request;
    this.tail = this.tail.then(() => write(value)).catch(() => {
      if (this.latest === request) onLatestFailure();
    });
    return this.tail;
  }
}

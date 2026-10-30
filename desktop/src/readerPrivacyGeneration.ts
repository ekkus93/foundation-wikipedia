/** Canonical durable SQLite privacy generation, serialized as a string
 * because the value can exceed JavaScript's safe integer range. */
export class ReaderPrivacyGenerationMonitor {
  private last: string | null = null;

  observe(value: unknown): "invalid" | "initial" | "unchanged" | "changed" | "stale" {
    if (typeof value !== "string" || !/^(0|[1-9][0-9]*)$/.test(value)
      || value.length > 19 || BigInt(value) > 9223372036854775807n) {
      return "invalid";
    }
    if (this.last === null) {
      this.last = value;
      return "initial";
    }
    if (this.last === value) return "unchanged";
    // Cross-process reads can complete out of order. A lower generation
    // cannot supersede a later, already-observed privacy reset.
    if (BigInt(value) < BigInt(this.last)) return "stale";
    this.last = value;
    return "changed";
  }
}

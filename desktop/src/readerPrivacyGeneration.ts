/** Canonical durable SQLite privacy generation, serialized as a string
 * because the value can exceed JavaScript's safe integer range. */
export class ReaderPrivacyGenerationMonitor {
  private last: string | null = null;

  observe(value: unknown, canApply = true): "invalid" | "initial" | "unchanged" | "changed" | "stale" | "deferred" {
    if (typeof value !== "string" || !/^(0|[1-9][0-9]*)$/.test(value)
      || value.length > 19 || BigInt(value) > 9223372036854775807n) {
      return "invalid";
    }
    if (this.last === null) {
      if (!canApply) return "deferred";
      this.last = value;
      return "initial";
    }
    if (this.last === value) return "unchanged";
    // Cross-process reads can complete out of order. A lower generation
    // cannot supersede a later, already-observed privacy reset.
    if (BigInt(value) < BigInt(this.last)) return "stale";
    // If a local deletion already owns the reset session, do not consume an
    // external change before the UI can invalidate its cached personal data.
    if (!canApply) return "deferred";
    this.last = value;
    return "changed";
  }
}

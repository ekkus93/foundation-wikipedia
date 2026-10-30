/** Canonical durable SQLite privacy generation, serialized as a string
 * because the value can exceed JavaScript's safe integer range. */
export class ReaderPrivacyGenerationMonitor {
  private last: string | null = null;

  observe(value: unknown): "invalid" | "initial" | "unchanged" | "changed" {
    if (typeof value !== "string" || !/^(0|[1-9][0-9]*)$/.test(value)
      || value.length > 19 || BigInt(value) > 9223372036854775807n) {
      return "invalid";
    }
    if (this.last === null) {
      this.last = value;
      return "initial";
    }
    if (this.last === value) return "unchanged";
    this.last = value;
    return "changed";
  }
}

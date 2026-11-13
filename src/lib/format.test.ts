import { describe, expect, it } from "vitest";
import { formatBytes, formatRelative, plural } from "./format";

describe("format", () => {
  it("formats bytes with decimal units", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(null)).toBe("0 B");
    expect(formatBytes(999)).toBe("999 B");
    expect(formatBytes(1500)).toBe("1.5 KB");
    expect(formatBytes(1_500_000)).toBe("1.5 MB");
    expect(formatBytes(250_000_000_000)).toBe("250 GB");
  });
  it("formats relative times", () => {
    const now = 1_000_000_000_000;
    expect(formatRelative(null, now)).toBe("Never");
    expect(formatRelative(now - 10_000, now)).toBe("Just now");
    expect(formatRelative(now - 5 * 60_000, now)).toBe("5 min ago");
    expect(formatRelative(now - 3 * 3_600_000, now)).toBe("3 hr ago");
    expect(formatRelative(now - 26 * 3_600_000, now)).toBe("Yesterday");
  });
  it("pluralises", () => {
    expect(plural(1, "file")).toBe("1 file");
    expect(plural(1200, "file")).toBe("1,200 files");
  });
});

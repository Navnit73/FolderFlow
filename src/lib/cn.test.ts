import { describe, expect, it } from "vitest";
import { cn } from "./cn";

describe("cn", () => {
  it("resolves conflicts between token spacing utilities", () => {
    expect(cn("h-control", "h-control-sm")).toBe("h-control-sm");
    expect(cn("p-page", "p-2")).toBe("p-2");
  });
  it("keeps token text color and text size together", () => {
    expect(cn("text-sm", "text-fg-muted")).toBe("text-sm text-fg-muted");
    expect(cn("text-fg", "text-fg-muted")).toBe("text-fg-muted");
  });
});

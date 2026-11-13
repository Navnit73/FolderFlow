import type { UiError } from "@/types/settings";

type CommandResult<T> = { status: "ok"; data: T } | { status: "error"; error: UiError };
// Commands returning `()` in Rust resolve to `null`.

export class CommandError extends Error {
  readonly ui: UiError;
  constructor(ui: UiError) {
    super(ui.message);
    this.name = "CommandError";
    this.ui = ui;
  }
}

/** Unwraps a tauri-specta result, throwing a typed CommandError on failure. */
export async function unwrap<T>(call: Promise<CommandResult<T>>): Promise<T> {
  const result = await call;
  if (result.status === "ok") return result.data;
  throw new CommandError(result.error);
}

/** Normalises anything thrown into a UiError the UI can render. */
export function toUiError(e: unknown): UiError {
  if (e instanceof CommandError) return e.ui;
  return {
    code: "internal",
    message: "Something went wrong inside FolderFlow.",
    hint: "Try again. If it keeps happening, check the log.",
    field: null,
  };
}

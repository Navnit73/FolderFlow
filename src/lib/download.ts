/** Triggers a browser/webview text download when running outside jsdom. */
export function triggerTextDownload(filename: string, content: string, mimeType: string): void {
  if (
    typeof URL !== "undefined" &&
    typeof URL.createObjectURL === "function" &&
    typeof navigator !== "undefined" &&
    !navigator.userAgent.includes("jsdom")
  ) {
    try {
      const blob = new Blob([content], { type: mimeType });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = filename;
      a.click();
      URL.revokeObjectURL(url);
    } catch {
      // Ignore browser download fallback errors in headless environments.
    }
  }
}


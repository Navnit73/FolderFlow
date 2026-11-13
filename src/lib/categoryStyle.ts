/**
 * Static class names for category color tokens, so Tailwind can see them.
 * Tokens are defined in src/styles/theme.css; categories store the token name.
 */
const SWATCH: Record<string, string> = {
  "cat-documents": "bg-cat-documents",
  "cat-images": "bg-cat-images",
  "cat-videos": "bg-cat-videos",
  "cat-audio": "bg-cat-audio",
  "cat-archives": "bg-cat-archives",
  "cat-spreadsheets": "bg-cat-spreadsheets",
  "cat-presentations": "bg-cat-presentations",
  "cat-installers": "bg-cat-installers",
  "cat-design": "bg-cat-design",
  "cat-development": "bg-cat-development",
  "cat-fonts": "bg-cat-fonts",
  "cat-ebooks": "bg-cat-ebooks",
  "cat-other": "bg-cat-other",
};

export function categorySwatch(colorToken: string | undefined): string {
  return SWATCH[colorToken ?? ""] ?? "bg-cat-other";
}

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

export const CATEGORY_COLOR_TOKENS: { token: string; label: string }[] = [
  { token: "cat-documents", label: "Documents" },
  { token: "cat-images", label: "Images" },
  { token: "cat-videos", label: "Videos" },
  { token: "cat-audio", label: "Audio" },
  { token: "cat-archives", label: "Archives" },
  { token: "cat-spreadsheets", label: "Spreadsheets" },
  { token: "cat-presentations", label: "Presentations" },
  { token: "cat-installers", label: "Installers" },
  { token: "cat-design", label: "Design" },
  { token: "cat-development", label: "Development" },
  { token: "cat-fonts", label: "Fonts" },
  { token: "cat-ebooks", label: "eBooks" },
  { token: "cat-other", label: "Other" },
];

export function categorySwatch(colorToken: string | undefined): string {
  return SWATCH[colorToken ?? ""] ?? "bg-cat-other";
}

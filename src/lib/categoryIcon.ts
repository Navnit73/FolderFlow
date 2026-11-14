import {
  Archive,
  BookOpen,
  Code,
  FileText,
  Film,
  Folder,
  Image,
  Music,
  Package,
  PenTool,
  Presentation,
  Receipt,
  Sheet,
  Sparkles,
  Tag,
  Type,
  type LucideIcon,
} from "lucide-react";

export const CATEGORY_ICON_OPTIONS: { id: string; label: string; Icon: LucideIcon }[] = [
  { id: "file-text", label: "Document", Icon: FileText },
  { id: "image", label: "Image", Icon: Image },
  { id: "film", label: "Video", Icon: Film },
  { id: "music", label: "Audio", Icon: Music },
  { id: "archive", label: "Archive", Icon: Archive },
  { id: "sheet", label: "Spreadsheet", Icon: Sheet },
  { id: "presentation", label: "Slides", Icon: Presentation },
  { id: "package", label: "Installer", Icon: Package },
  { id: "pen-tool", label: "Design", Icon: PenTool },
  { id: "code", label: "Code", Icon: Code },
  { id: "type", label: "Font", Icon: Type },
  { id: "book-open", label: "eBook", Icon: BookOpen },
  { id: "receipt", label: "Receipt", Icon: Receipt },
  { id: "sparkles", label: "Smart", Icon: Sparkles },
  { id: "tag", label: "Tag", Icon: Tag },
  { id: "folder", label: "Folder", Icon: Folder },
];

const ICON_MAP: Record<string, LucideIcon> = Object.fromEntries(
  CATEGORY_ICON_OPTIONS.map((item) => [item.id, item.Icon]),
);

export function categoryIcon(name: string | undefined): LucideIcon {
  return ICON_MAP[name ?? ""] ?? Tag;
}


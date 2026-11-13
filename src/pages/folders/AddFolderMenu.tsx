import { Download, Film, FolderPlus, FolderSearch, Image, Monitor, Music, FileText, Plus } from "lucide-react";
import { DropdownMenu } from "@/components/ui/dropdown-menu";
import { useFolders } from "@/stores/folders";

export function AddFolderMenu({ variant = "primary" }: { variant?: "primary" | "secondary" }) {
  const addKnown = useFolders((s) => s.addKnown);
  const addWithPicker = useFolders((s) => s.addWithPicker);
  return (
    <DropdownMenu
      variant={variant}
      label={
        <>
          <Plus aria-hidden />
          Add folder
        </>
      }
      items={[
        { label: "Downloads", icon: Download, onSelect: () => void addKnown("downloads") },
        { label: "Desktop", icon: Monitor, onSelect: () => void addKnown("desktop") },
        { label: "Documents", icon: FileText, onSelect: () => void addKnown("documents") },
        { label: "Pictures", icon: Image, onSelect: () => void addKnown("pictures") },
        { label: "Movies", icon: Film, onSelect: () => void addKnown("movies") },
        { label: "Music", icon: Music, onSelect: () => void addKnown("music") },
        { label: "Choose folder…", icon: FolderSearch, onSelect: () => void addWithPicker(), separatorBefore: true },
      ]}
    />
  );
}

export const AddFolderIcon = FolderPlus;

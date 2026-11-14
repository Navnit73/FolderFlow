import { ArrowDown, ArrowUp, FolderSearch, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { rulesService, type Action, type DatePlacement, type DateSource, type Rule } from "@/services/rules";
import { TemplateInput } from "./TemplateInput";

const selectClass =
  "h-control rounded-md border border-border-strong bg-surface px-2 text-sm text-fg focus-visible:border-border-focus";
const inputClass =
  "h-control rounded-md border border-border-strong bg-surface px-2 text-sm text-fg placeholder:text-fg-subtle focus-visible:border-border-focus";

export function defaultActionForKind(kind: Action["kind"]): Action {
  switch (kind) {
    case "move_into_dynamic_folder":
      return {
        kind: "move_into_dynamic_folder",
        root: null,
        subpath_template: "Documents/{year}/{month_name}",
      };
    case "move":
      return { kind: "move", destination: "" };
    case "copy":
      return { kind: "copy", destination: "" };
    case "rename":
      return { kind: "rename", template: "{date}_{original_name}" };
    case "add_prefix":
      return { kind: "add_prefix", prefix: "sorted_" };
    case "add_suffix":
      return { kind: "add_suffix", suffix: "_archived" };
    case "remove_text":
      return { kind: "remove_text", text: "Copy of ", case_sensitive: false };
    case "replace_text":
      return { kind: "replace_text", find: " ", replace: "_", case_sensitive: false };
    case "add_date_to_filename":
      return { kind: "add_date_to_filename", placement: "prefix", format: "{date}" };
    case "change_extension":
      return { kind: "change_extension", new_extension: "txt", confirm_unsafe: false };
    case "create_destination_folder":
      return { kind: "create_destination_folder", path: "" };
    case "apply_another_rule":
      return { kind: "apply_another_rule", rule_id: "" };
    case "notify":
      return { kind: "notify", message: "Organized file" };
    case "mark_for_review":
      return { kind: "mark_for_review", reason: "Verify before moving" };
    case "skip":
      return { kind: "skip" };
    case "stop_processing":
      return { kind: "stop_processing" };
  }
}

interface ActionRowProps {
  index: number;
  action: Action;
  dateSource: DateSource;
  otherRules: Rule[];
  canMoveUp?: boolean;
  canMoveDown?: boolean;
  canRemove: boolean;
  onChange: (next: Action) => void;
  onMoveUp?: () => void;
  onMoveDown?: () => void;
  onRemove?: () => void;
}

export function ActionRow({
  index,
  action,
  dateSource,
  otherRules,
  canMoveUp = false,
  canMoveDown = false,
  canRemove,
  onChange,
  onMoveUp,
  onMoveDown,
  onRemove,
}: ActionRowProps) {
  const pickFolder = async (applyPath: (path: string) => void) => {
    try {
      const picked = await rulesService.pickDestinationFolder();
      if (picked) applyPath(picked);
    } catch {
      // Handled by Rust error channel if needed
    }
  };

  return (
    <div className="space-y-2.5 rounded-md border border-border bg-surface p-3">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-xs font-medium text-fg-muted">#{index + 1}</span>
        <select
          aria-label="Action type"
          value={action.kind}
          onChange={(e) => onChange(defaultActionForKind(e.target.value as Action["kind"]))}
          className={selectClass}
        >
          <option value="move_into_dynamic_folder">Move into dynamic folder</option>
          <option value="move">Move to folder</option>
          <option value="copy">Copy to folder</option>
          <option value="rename">Rename with template</option>
          <option value="add_prefix">Add prefix to filename</option>
          <option value="add_suffix">Add suffix to filename</option>
          <option value="remove_text">Remove text from filename</option>
          <option value="replace_text">Replace text in filename</option>
          <option value="add_date_to_filename">Add date to filename</option>
          <option value="change_extension">Change extension</option>
          <option value="create_destination_folder">Create destination folder</option>
          <option value="apply_another_rule">Apply another rule</option>
          <option value="notify">Send notification</option>
          <option value="mark_for_review">Mark for review</option>
          <option value="skip">Skip file</option>
          <option value="stop_processing">Stop processing rules</option>
        </select>

        <div className="ml-auto flex items-center gap-0.5">
          {onMoveUp && (
            <Button
              size="icon"
              variant="ghost"
              disabled={!canMoveUp}
              aria-label="Move action up"
              title="Move action up"
              onClick={onMoveUp}
            >
              <ArrowUp />
            </Button>
          )}
          {onMoveDown && (
            <Button
              size="icon"
              variant="ghost"
              disabled={!canMoveDown}
              aria-label="Move action down"
              title="Move action down"
              onClick={onMoveDown}
            >
              <ArrowDown />
            </Button>
          )}
          {onRemove && (
            <Button
              size="icon"
              variant="ghost"
              disabled={!canRemove}
              aria-label="Remove action"
              title="Remove action"
              onClick={onRemove}
            >
              <Trash2 />
            </Button>
          )}
        </div>
      </div>

      {action.kind === "move_into_dynamic_folder" && (
        <div className="space-y-2">
          <div className="flex flex-wrap items-center gap-2">
            <input
              type="text"
              aria-label="Base destination root"
              placeholder="Source folder (default) or choose base folder…"
              value={action.root ?? ""}
              onChange={(e) =>
                onChange({
                  ...action,
                  root: e.target.value.trim() ? e.target.value : null,
                })
              }
              className={`${inputClass} flex-1`}
            />
            <Button
              size="sm"
              onClick={() =>
                void pickFolder((picked) => onChange({ ...action, root: picked }))
              }
            >
              <FolderSearch aria-hidden />
              Choose folder…
            </Button>
          </div>
          <TemplateInput
            id={`action-subpath-${index}`}
            label="Subfolder template"
            value={action.subpath_template}
            onChange={(subpath_template) => onChange({ ...action, subpath_template })}
            dateSource={dateSource}
            placeholder="Invoices/{year}/{month_name}"
          />
        </div>
      )}

      {action.kind === "move" && (
        <div className="flex flex-wrap items-center gap-2">
          <input
            type="text"
            aria-label="Destination folder"
            placeholder="/Users/you/Documents/Sorted"
            value={action.destination}
            onChange={(e) => onChange({ kind: "move", destination: e.target.value })}
            className={`${inputClass} flex-1`}
          />
          <Button
            size="sm"
            onClick={() =>
              void pickFolder((destination) => onChange({ kind: "move", destination }))
            }
          >
            <FolderSearch aria-hidden />
            Choose folder…
          </Button>
        </div>
      )}

      {action.kind === "copy" && (
        <div className="flex flex-wrap items-center gap-2">
          <input
            type="text"
            aria-label="Copy destination folder"
            placeholder="/Users/you/Documents/Backup"
            value={action.destination}
            onChange={(e) => onChange({ kind: "copy", destination: e.target.value })}
            className={`${inputClass} flex-1`}
          />
          <Button
            size="sm"
            onClick={() =>
              void pickFolder((destination) => onChange({ kind: "copy", destination }))
            }
          >
            <FolderSearch aria-hidden />
            Choose folder…
          </Button>
        </div>
      )}

      {action.kind === "rename" && (
        <TemplateInput
          id={`action-rename-${index}`}
          label="Filename template"
          value={action.template}
          onChange={(template) => onChange({ kind: "rename", template })}
          isFilename
          dateSource={dateSource}
          placeholder="{date}_{original_name}"
        />
      )}

      {action.kind === "add_prefix" && (
        <input
          type="text"
          aria-label="Filename prefix"
          placeholder="paid_"
          value={action.prefix}
          onChange={(e) => onChange({ kind: "add_prefix", prefix: e.target.value })}
          className={`${inputClass} w-full`}
        />
      )}

      {action.kind === "add_suffix" && (
        <input
          type="text"
          aria-label="Filename suffix"
          placeholder="_final"
          value={action.suffix}
          onChange={(e) => onChange({ kind: "add_suffix", suffix: e.target.value })}
          className={`${inputClass} w-full`}
        />
      )}

      {action.kind === "remove_text" && (
        <div className="flex flex-wrap items-center gap-2">
          <input
            type="text"
            aria-label="Text to remove"
            placeholder="Copy of "
            value={action.text}
            onChange={(e) => onChange({ ...action, text: e.target.value })}
            className={`${inputClass} flex-1`}
          />
          <label className="flex items-center gap-1.5 text-xs text-fg-muted">
            <input
              type="checkbox"
              checked={action.case_sensitive}
              onChange={(e) => onChange({ ...action, case_sensitive: e.target.checked })}
            />
            Case sensitive
          </label>
        </div>
      )}

      {action.kind === "replace_text" && (
        <div className="flex flex-wrap items-center gap-2">
          <input
            type="text"
            aria-label="Find text"
            placeholder="Find"
            value={action.find}
            onChange={(e) => onChange({ ...action, find: e.target.value })}
            className={`${inputClass} flex-1`}
          />
          <input
            type="text"
            aria-label="Replace with"
            placeholder="Replace with"
            value={action.replace}
            onChange={(e) => onChange({ ...action, replace: e.target.value })}
            className={`${inputClass} flex-1`}
          />
          <label className="flex items-center gap-1.5 text-xs text-fg-muted">
            <input
              type="checkbox"
              checked={action.case_sensitive}
              onChange={(e) => onChange({ ...action, case_sensitive: e.target.checked })}
            />
            Case sensitive
          </label>
        </div>
      )}

      {action.kind === "add_date_to_filename" && (
        <div className="flex flex-wrap items-center gap-2">
          <select
            aria-label="Date placement"
            value={action.placement}
            onChange={(e) =>
              onChange({ ...action, placement: e.target.value as DatePlacement })
            }
            className={selectClass}
          >
            <option value="prefix">As prefix</option>
            <option value="suffix">As suffix</option>
          </select>
          <input
            type="text"
            aria-label="Date format template"
            value={action.format}
            onChange={(e) => onChange({ ...action, format: e.target.value })}
            className={`${inputClass} flex-1 font-mono`}
          />
        </div>
      )}

      {action.kind === "change_extension" && (
        <div className="flex flex-wrap items-center gap-3">
          <input
            type="text"
            aria-label="New extension"
            placeholder="pdf"
            value={action.new_extension}
            onChange={(e) =>
              onChange({ ...action, new_extension: e.target.value.replace(/^\./, "") })
            }
            className={`${inputClass} w-36`}
          />
          <label className="flex items-center gap-1.5 text-xs text-fg-muted">
            <input
              type="checkbox"
              checked={action.confirm_unsafe}
              onChange={(e) => onChange({ ...action, confirm_unsafe: e.target.checked })}
            />
            Confirm changing file extension
          </label>
        </div>
      )}

      {action.kind === "create_destination_folder" && (
        <div className="flex flex-wrap items-center gap-2">
          <input
            type="text"
            aria-label="Folder path to create"
            placeholder="/Users/you/Documents/Archive"
            value={action.path}
            onChange={(e) =>
              onChange({ kind: "create_destination_folder", path: e.target.value })
            }
            className={`${inputClass} flex-1`}
          />
          <Button
            size="sm"
            onClick={() =>
              void pickFolder((path) =>
                onChange({ kind: "create_destination_folder", path }),
              )
            }
          >
            <FolderSearch aria-hidden />
            Choose folder…
          </Button>
        </div>
      )}

      {action.kind === "apply_another_rule" && (
        <select
          aria-label="Target rule"
          value={action.rule_id}
          onChange={(e) => onChange({ kind: "apply_another_rule", rule_id: e.target.value })}
          className={`${selectClass} w-full`}
        >
          <option value="">Select another rule…</option>
          {otherRules.map((r) => (
            <option key={r.id} value={r.id}>
              {r.name}
            </option>
          ))}
        </select>
      )}

      {action.kind === "notify" && (
        <input
          type="text"
          aria-label="Notification message"
          placeholder="Moved invoice to archive"
          value={action.message}
          onChange={(e) => onChange({ kind: "notify", message: e.target.value })}
          className={`${inputClass} w-full`}
        />
      )}

      {action.kind === "mark_for_review" && (
        <input
          type="text"
          aria-label="Review reason"
          placeholder="Reason for manual review"
          value={action.reason}
          onChange={(e) => onChange({ kind: "mark_for_review", reason: e.target.value })}
          className={`${inputClass} w-full`}
        />
      )}
    </div>
  );
}

import { ArrowDown, ArrowUp, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import type {
  Condition,
  DateConditionOp,
  DateField,
  LocationOp,
  SizeOp,
  SizeUnit,
  TextFieldKind,
  TextOp,
} from "@/services/rules";

const selectClass =
  "h-control rounded-md border border-border-strong bg-surface px-2 text-sm text-fg focus-visible:border-border-focus";
const inputClass =
  "h-control rounded-md border border-border-strong bg-surface px-2 text-sm text-fg placeholder:text-fg-subtle focus-visible:border-border-focus";

const BUILTIN_CATEGORIES = [
  { id: "documents", label: "Documents" },
  { id: "images", label: "Images" },
  { id: "videos", label: "Videos" },
  { id: "audio", label: "Audio" },
  { id: "archives", label: "Archives" },
  { id: "spreadsheets", label: "Spreadsheets" },
  { id: "presentations", label: "Presentations" },
  { id: "installers", label: "Applications & Installers" },
  { id: "design", label: "Design" },
  { id: "development", label: "Development" },
  { id: "fonts", label: "Fonts" },
  { id: "ebooks", label: "E-books" },
  { id: "other", label: "Other" },
] as const;

export function defaultConditionForKind(kind: Condition["kind"]): Condition {
  switch (kind) {
    case "text":
      return { kind: "text", field: "name", op: "contains", value: "", case_sensitive: false };
    case "category":
      return { kind: "category", category_id: "documents", negate: false };
    case "size":
      return { kind: "size", op: "greater_than", value: 10, max_value: null, unit: "megabytes" };
    case "date":
      return {
        kind: "date",
        field: "modified",
        op: { op: "older_than_days", days: 30 },
      };
    case "location":
      return {
        kind: "location",
        op: { op: "source_folder_equals", folder: "Downloads" },
      };
    case "hidden":
      return { kind: "hidden", is_hidden: false };
    case "empty_file":
      return { kind: "empty_file", is_empty: true };
    case "duplicate_status":
      return { kind: "duplicate_status", is_duplicate: true };
    case "content_hash":
      return { kind: "content_hash", hash: "" };
    case "filename_glob":
      return { kind: "filename_glob", pattern: "*.pdf", case_sensitive: false };
    case "extension_group":
      return { kind: "extension_group", extensions: ["pdf"] };
  }
}

function defaultDateOp(opKind: DateConditionOp["op"]): DateConditionOp {
  switch (opKind) {
    case "before":
      return { op: "before", date: "2026-01-01" };
    case "after":
      return { op: "after", date: "2026-01-01" };
    case "within_range":
      return { op: "within_range", start: "2026-01-01", end: "2026-12-31" };
    case "older_than_days":
      return { op: "older_than_days", days: 30 };
    case "newer_than_days":
      return { op: "newer_than_days", days: 7 };
    case "today":
      return { op: "today" };
    case "yesterday":
      return { op: "yesterday" };
    case "this_week":
      return { op: "this_week" };
    case "this_month":
      return { op: "this_month" };
    case "this_year":
      return { op: "this_year" };
  }
}

function defaultLocationOp(opKind: LocationOp["op"]): LocationOp {
  switch (opKind) {
    case "source_folder_equals":
      return { op: "source_folder_equals", folder: "Downloads" };
    case "source_folder_contains":
      return { op: "source_folder_contains", text: "Download" };
    case "inside_directory":
      return { op: "inside_directory", path: "/Users/Shared" };
    case "inside_subdirectory":
      return { op: "inside_subdirectory", subpath: null };
  }
}

interface ConditionRowProps {
  condition: Condition;
  onChange: (next: Condition) => void;
  onMoveUp?: () => void;
  onMoveDown?: () => void;
  onRemove?: () => void;
  canMoveUp?: boolean;
  canMoveDown?: boolean;
  canRemove: boolean;
}

export function ConditionRow({
  condition,
  onChange,
  onMoveUp,
  onMoveDown,
  onRemove,
  canMoveUp = false,
  canMoveDown = false,
  canRemove,
}: ConditionRowProps) {
  return (
    <div className="space-y-1.5 rounded-md border border-border bg-surface p-2.5">
      <div className="flex flex-wrap items-center gap-2">
        <select
          aria-label="Condition type"
          value={condition.kind}
          onChange={(e) => onChange(defaultConditionForKind(e.target.value as Condition["kind"]))}
          className={selectClass}
        >
          <option value="text">Text / Filename</option>
          <option value="extension_group">Extension group</option>
          <option value="category">Category</option>
          <option value="size">File size</option>
          <option value="date">Date</option>
          <option value="location">Location</option>
          <option value="filename_glob">Filename pattern (glob)</option>
          <option value="hidden">Hidden file</option>
          <option value="empty_file">Empty file</option>
          <option value="duplicate_status">Duplicate status</option>
          <option value="content_hash">Content hash (BLAKE3)</option>
        </select>

        {condition.kind === "text" && (
          <>
            <select
              aria-label="Text attribute"
              value={condition.field}
              onChange={(e) =>
                onChange({ ...condition, field: e.target.value as TextFieldKind })
              }
              className={selectClass}
            >
              <option value="name">Filename</option>
              <option value="stem">Filename stem</option>
              <option value="extension">Extension</option>
              <option value="mime_type">MIME type</option>
              <option value="parent_folder">Parent folder</option>
              <option value="full_path">Full path</option>
            </select>
            <select
              aria-label="Text operator"
              value={condition.op}
              onChange={(e) => onChange({ ...condition, op: e.target.value as TextOp })}
              className={selectClass}
            >
              <option value="contains">contains</option>
              <option value="not_contains">does not contain</option>
              <option value="starts_with">starts with</option>
              <option value="ends_with">ends with</option>
              <option value="equals">equals</option>
              <option value="not_equals">does not equal</option>
              <option value="matches_regex">matches regex</option>
            </select>
            <input
              type="text"
              aria-label="Text value"
              placeholder={condition.op === "matches_regex" ? "^INV-\\d+" : "invoice"}
              value={condition.value}
              onChange={(e) => onChange({ ...condition, value: e.target.value })}
              className={`${inputClass} min-w-36 flex-1`}
            />
            <label className="flex items-center gap-1.5 text-xs text-fg-muted">
              <input
                type="checkbox"
                checked={condition.case_sensitive}
                onChange={(e) => onChange({ ...condition, case_sensitive: e.target.checked })}
              />
              Case sensitive
            </label>
          </>
        )}

        {condition.kind === "extension_group" && (
          <input
            type="text"
            aria-label="Extensions list"
            placeholder="pdf, docx, txt"
            value={condition.extensions.join(", ")}
            onChange={(e) =>
              onChange({
                kind: "extension_group",
                extensions: e.target.value
                  .split(/[,\s]+/)
                  .map((s) => s.trim().replace(/^\./, ""))
                  .filter(Boolean),
              })
            }
            className={`${inputClass} min-w-44 flex-1`}
          />
        )}

        {condition.kind === "category" && (
          <>
            <select
              aria-label="Category match mode"
              value={condition.negate ? "is_not" : "is"}
              onChange={(e) =>
                onChange({ ...condition, negate: e.target.value === "is_not" })
              }
              className={selectClass}
            >
              <option value="is">is</option>
              <option value="is_not">is not</option>
            </select>
            <select
              aria-label="Category value"
              value={condition.category_id}
              onChange={(e) => onChange({ ...condition, category_id: e.target.value })}
              className={selectClass}
            >
              {BUILTIN_CATEGORIES.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.label}
                </option>
              ))}
            </select>
          </>
        )}

        {condition.kind === "size" && (
          <>
            <select
              aria-label="Size operator"
              value={condition.op}
              onChange={(e) => {
                const nextOp = e.target.value as SizeOp;
                onChange({
                  ...condition,
                  op: nextOp,
                  max_value: nextOp === "between" ? (condition.max_value ?? condition.value + 10) : null,
                });
              }}
              className={selectClass}
            >
              <option value="greater_than">is greater than</option>
              <option value="less_than">is less than</option>
              <option value="equals">equals</option>
              <option value="between">is between</option>
            </select>
            <input
              type="number"
              min={0}
              aria-label="Size value"
              value={condition.value}
              onChange={(e) => onChange({ ...condition, value: Number(e.target.value) || 0 })}
              className={`${inputClass} w-24`}
            />
            {condition.op === "between" && (
              <>
                <span className="text-xs text-fg-muted">and</span>
                <input
                  type="number"
                  min={0}
                  aria-label="Maximum size value"
                  value={condition.max_value ?? 0}
                  onChange={(e) =>
                    onChange({ ...condition, max_value: Number(e.target.value) || 0 })
                  }
                  className={`${inputClass} w-24`}
                />
              </>
            )}
            <select
              aria-label="Size unit"
              value={condition.unit}
              onChange={(e) => onChange({ ...condition, unit: e.target.value as SizeUnit })}
              className={selectClass}
            >
              <option value="bytes">Bytes (B)</option>
              <option value="kilobytes">KB</option>
              <option value="megabytes">MB</option>
              <option value="gigabytes">GB</option>
            </select>
          </>
        )}

        {condition.kind === "date" && (
          <>
            <select
              aria-label="Date field"
              value={condition.field}
              onChange={(e) => onChange({ ...condition, field: e.target.value as DateField })}
              className={selectClass}
            >
              <option value="modified">Modified date</option>
              <option value="created">Created date</option>
              <option value="added">Added date</option>
            </select>
            <select
              aria-label="Date operator"
              value={condition.op.op}
              onChange={(e) =>
                onChange({
                  ...condition,
                  op: defaultDateOp(e.target.value as DateConditionOp["op"]),
                })
              }
              className={selectClass}
            >
              <option value="older_than_days">is older than (days)</option>
              <option value="newer_than_days">is newer than (days)</option>
              <option value="before">is before date</option>
              <option value="after">is after date</option>
              <option value="within_range">is within range</option>
              <option value="today">is today</option>
              <option value="yesterday">is yesterday</option>
              <option value="this_week">is this week</option>
              <option value="this_month">is this month</option>
              <option value="this_year">is this year</option>
            </select>
            {(condition.op.op === "older_than_days" || condition.op.op === "newer_than_days") && (
              <input
                type="number"
                min={0}
                aria-label="Days count"
                value={condition.op.days}
                onChange={(e) =>
                  onChange({
                    ...condition,
                    op: { op: condition.op.op as "older_than_days" | "newer_than_days", days: Number(e.target.value) || 0 },
                  })
                }
                className={`${inputClass} w-24`}
              />
            )}
            {(condition.op.op === "before" || condition.op.op === "after") && (
              <input
                type="text"
                aria-label="Date value"
                placeholder="YYYY-MM-DD"
                value={condition.op.date}
                onChange={(e) =>
                  onChange({
                    ...condition,
                    op: { op: condition.op.op as "before" | "after", date: e.target.value },
                  })
                }
                className={`${inputClass} w-36`}
              />
            )}
            {condition.op.op === "within_range" && (
              <>
                <input
                  type="text"
                  aria-label="Start date"
                  placeholder="YYYY-MM-DD"
                  value={condition.op.start}
                  onChange={(e) => {
                    if (condition.op.op !== "within_range") return;
                    onChange({
                      ...condition,
                      op: { op: "within_range", start: e.target.value, end: condition.op.end },
                    });
                  }}
                  className={`${inputClass} w-32`}
                />
                <span className="text-xs text-fg-muted">to</span>
                <input
                  type="text"
                  aria-label="End date"
                  placeholder="YYYY-MM-DD"
                  value={condition.op.end}
                  onChange={(e) => {
                    if (condition.op.op !== "within_range") return;
                    onChange({
                      ...condition,
                      op: { op: "within_range", start: condition.op.start, end: e.target.value },
                    });
                  }}
                  className={`${inputClass} w-32`}
                />
              </>
            )}
          </>
        )}

        {condition.kind === "location" && (
          <>
            <select
              aria-label="Location operator"
              value={condition.op.op}
              onChange={(e) =>
                onChange({
                  kind: "location",
                  op: defaultLocationOp(e.target.value as LocationOp["op"]),
                })
              }
              className={selectClass}
            >
              <option value="source_folder_equals">Source folder equals</option>
              <option value="source_folder_contains">Source folder contains</option>
              <option value="inside_directory">Inside directory</option>
              <option value="inside_subdirectory">Inside subdirectory</option>
            </select>
            {condition.op.op === "source_folder_equals" && (
              <input
                type="text"
                aria-label="Location value"
                value={condition.op.folder}
                onChange={(e) =>
                  onChange({
                    kind: "location",
                    op: { op: "source_folder_equals", folder: e.target.value },
                  })
                }
                className={`${inputClass} min-w-36 flex-1`}
              />
            )}
            {condition.op.op === "source_folder_contains" && (
              <input
                type="text"
                aria-label="Location value"
                value={condition.op.text}
                onChange={(e) =>
                  onChange({
                    kind: "location",
                    op: { op: "source_folder_contains", text: e.target.value },
                  })
                }
                className={`${inputClass} min-w-36 flex-1`}
              />
            )}
            {condition.op.op === "inside_directory" && (
              <input
                type="text"
                aria-label="Location value"
                value={condition.op.path}
                onChange={(e) =>
                  onChange({
                    kind: "location",
                    op: { op: "inside_directory", path: e.target.value },
                  })
                }
                className={`${inputClass} min-w-36 flex-1`}
              />
            )}
            {condition.op.op === "inside_subdirectory" && (
              <input
                type="text"
                aria-label="Subdirectory subpath"
                placeholder="Any subfolder (optional subpath)"
                value={condition.op.subpath ?? ""}
                onChange={(e) =>
                  onChange({
                    kind: "location",
                    op: {
                      op: "inside_subdirectory",
                      subpath: e.target.value.trim() ? e.target.value : null,
                    },
                  })
                }
                className={`${inputClass} min-w-36 flex-1`}
              />
            )}
          </>
        )}

        {condition.kind === "filename_glob" && (
          <>
            <input
              type="text"
              aria-label="Glob pattern"
              placeholder="Screen Shot *.png"
              value={condition.pattern}
              onChange={(e) => onChange({ ...condition, pattern: e.target.value })}
              className={`${inputClass} min-w-44 flex-1`}
            />
            <label className="flex items-center gap-1.5 text-xs text-fg-muted">
              <input
                type="checkbox"
                checked={condition.case_sensitive}
                onChange={(e) => onChange({ ...condition, case_sensitive: e.target.checked })}
              />
              Case sensitive
            </label>
          </>
        )}

        {condition.kind === "hidden" && (
          <select
            aria-label="Hidden status"
            value={condition.is_hidden ? "yes" : "no"}
            onChange={(e) => onChange({ kind: "hidden", is_hidden: e.target.value === "yes" })}
            className={selectClass}
          >
            <option value="yes">is hidden</option>
            <option value="no">is not hidden</option>
          </select>
        )}

        {condition.kind === "empty_file" && (
          <select
            aria-label="Empty file status"
            value={condition.is_empty ? "yes" : "no"}
            onChange={(e) => onChange({ kind: "empty_file", is_empty: e.target.value === "yes" })}
            className={selectClass}
          >
            <option value="yes">is empty (0 bytes)</option>
            <option value="no">is not empty</option>
          </select>
        )}

        {condition.kind === "duplicate_status" && (
          <select
            aria-label="Duplicate status"
            value={condition.is_duplicate ? "yes" : "no"}
            onChange={(e) =>
              onChange({ kind: "duplicate_status", is_duplicate: e.target.value === "yes" })
            }
            className={selectClass}
          >
            <option value="yes">is a duplicate</option>
            <option value="no">is unique</option>
          </select>
        )}

        {condition.kind === "content_hash" && (
          <input
            type="text"
            aria-label="BLAKE3 hash"
            placeholder="64-character hex BLAKE3 digest"
            value={condition.hash}
            onChange={(e) => onChange({ kind: "content_hash", hash: e.target.value })}
            className={`${inputClass} min-w-56 flex-1 font-mono`}
          />
        )}

        <div className="ml-auto flex items-center gap-0.5">
          {onMoveUp && (
            <Button
              size="icon"
              variant="ghost"
              disabled={!canMoveUp}
              aria-label="Move condition up"
              title="Move condition up"
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
              aria-label="Move condition down"
              title="Move condition down"
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
              aria-label="Remove condition"
              title="Remove condition"
              onClick={onRemove}
            >
              <Trash2 />
            </Button>
          )}
        </div>
      </div>

      {condition.kind === "size" && (
        <p className="text-2xs text-fg-muted">
          Decimal units (macOS Finder standard): 1 KB = 1000 B, 1 MB = 1,000,000 B.
        </p>
      )}
    </div>
  );
}

import { useEffect, useState } from "react";
import { cn } from "@/lib/cn";
import { rulesService, type DateSource, type TemplatePreviewOutput } from "@/services/rules";

const TEMPLATE_VARIABLES = [
  "{year}",
  "{month}",
  "{month_name}",
  "{day}",
  "{date}",
  "{extension}",
  "{category}",
  "{original_name}",
  "{source_folder}",
] as const;

const NAMING_PRESETS = [
  "{date}_{original_name}",
  "{year}-{month}_{original_name}",
  "{category}_{date}_{original_name}",
  "{source_folder}_{original_name}",
] as const;

const SAMPLE_FILE = {
  name: "invoice.pdf",
  category: "Documents",
  sourceFolder: "Downloads",
  modifiedMs: 1776249600000, // 2026-04-15T10:40:00Z
  createdMs: 1776249600000,
  addedMs: 1776249600000,
};

interface TemplateInputProps {
  id: string;
  label: string;
  value: string;
  onChange: (next: string) => void;
  isFilename?: boolean;
  dateSource: DateSource;
  placeholder?: string;
}

export function TemplateInput({
  id,
  label,
  value,
  onChange,
  isFilename = false,
  dateSource,
  placeholder,
}: TemplateInputProps) {
  const [preview, setPreview] = useState<TemplatePreviewOutput | null>(null);

  useEffect(() => {
    let active = true;
    rulesService
      .templatePreview({
        template: value,
        isFilename,
        dateSource,
        sample: SAMPLE_FILE,
      })
      .then(
        (res) => {
          if (active) setPreview(res);
        },
        () => {
          if (active) setPreview(null);
        },
      );
    return () => {
      active = false;
    };
  }, [value, isFilename, dateSource]);

  const insertVariable = (token: string) => {
    const next = value.endsWith("/") || value.length === 0 ? `${value}${token}` : `${value}${token}`;
    onChange(next);
  };

  return (
    <div className="space-y-1.5">
      <label htmlFor={id} className="block text-xs font-medium text-fg-muted">
        {label}
      </label>
      <input
        id={id}
        type="text"
        value={value}
        placeholder={placeholder}
        onChange={(e) => onChange(e.target.value)}
        className={cn(
          "h-control w-full rounded-md border bg-surface px-2 font-mono text-xs text-fg placeholder:text-fg-subtle focus-visible:border-border-focus",
          preview && !preview.valid ? "border-danger" : "border-border-strong",
        )}
      />

      {isFilename && (
        <div className="flex flex-wrap items-center gap-1 pt-0.5">
          <span className="text-2xs text-fg-subtle">Presets:</span>
          {NAMING_PRESETS.map((preset) => (
            <button
              key={preset}
              type="button"
              aria-label={`Insert preset ${preset}`}
              onClick={() => onChange(preset)}
              className="rounded-sm border border-border bg-muted px-1.5 py-0.5 font-mono text-2xs text-fg hover:bg-hover"
            >
              {preset}
            </button>
          ))}
        </div>
      )}

      <div className="flex flex-wrap items-center gap-1">
        <span className="text-2xs text-fg-subtle">Variables:</span>
        {TEMPLATE_VARIABLES.map((token) => (
          <button
            key={token}
            type="button"
            aria-label={`Insert variable ${token}`}
            onClick={() => insertVariable(token)}
            className="rounded-sm bg-muted px-1.5 py-0.5 font-mono text-2xs text-fg-muted hover:bg-hover hover:text-fg"
          >
            {token}
          </button>
        ))}
      </div>

      {preview && (
        <div className="text-xs">
          {preview.valid ? (
            <p className="text-fg-muted">
              Preview:{" "}
              <span className="rounded-sm bg-subtle px-1.5 py-0.5 font-mono text-fg">
                {preview.rendered}
              </span>
            </p>
          ) : (
            <p className="text-danger">{preview.error}</p>
          )}
        </div>
      )}
    </div>
  );
}


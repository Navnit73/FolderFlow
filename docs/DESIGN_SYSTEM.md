# FolderFlow — Design System

> **One file controls the entire UI: [`src/styles/theme.css`](../src/styles/theme.css).** Change a value there and every screen updates. Nothing else in the codebase may contain a color, font size, radius, shadow or arbitrary pixel value.

## 1. Direction

Native-macOS-inspired, minimal, flat, premium. Think Finder/System Settings, not a SaaS dashboard.

- System font (SF Pro), 13 px body — the macOS default.
- One accent (calm blue). Color is for meaning (status, category), not decoration.
- Flat surfaces separated by 1 px borders, not shadows. Shadows only on popovers/dialogs.
- Small radii (4–8 px).
- No gradients, no glassmorphism, no card-in-card, no oversized hero numbers outside the dashboard.
- Dense but breathable; two densities (comfortable / compact).

## 2. How theming works

```
theme.css
 ├─ 1. Primitives        --ff-gray-*, --ff-blue-* …         (never used by components)
 ├─ 2. Light semantic    --ff-bg, --ff-fg, --ff-accent …
 ├─ 3. Dark semantic     [data-theme=dark] + system-dark media query
 ├─ 4. Type/shape/motion/layout/density
 ├─ 5. @theme inline     maps tokens → Tailwind utilities (default palette removed)
 └─ 6. Base styles
```

| Mode | `<html>` attributes |
|---|---|
| System (default) | no `data-theme` |
| Light | `data-theme="light"` |
| Dark | `data-theme="dark"` |
| Density | `data-density="comfortable"` (default) / `"compact"` |

The Appearance settings page writes these attributes; nothing else does.

## 3. Token reference (use these utilities)

| Purpose | Utility | Token |
|---|---|---|
| Content background | `bg-surface` | `--ff-bg` |
| Sidebar | `bg-sidebar` | `--ff-bg-sidebar` |
| Inset / table header | `bg-subtle` | `--ff-bg-subtle` |
| Input / chip | `bg-muted` | `--ff-bg-muted` |
| Hover / pressed / selected | `bg-hover` `bg-active` `bg-selected` | |
| Primary text | `text-fg` | `--ff-fg` |
| Secondary / tertiary text | `text-fg-muted` `text-fg-subtle` | |
| Borders | `border-border` `border-border-strong` | |
| Primary button | `bg-accent text-fg-on-accent hover:bg-accent-hover` | |
| Link | `text-accent-fg` | |
| Status | `text-success` `bg-success-subtle` (also warning, danger, info) | |
| Monitoring dot | `bg-status-active/paused/error/idle` | |
| Category | `text-cat-images`, `bg-cat-documents` … | `--ff-cat-*` |
| Radius | `rounded-sm/md/lg` | |
| Heights | `h-control` `h-control-sm` `h-row` `h-header` | density-aware |
| Page padding | `p-page` | density-aware |
| Shadows | `shadow-popover` `shadow-dialog` | |

Category colors are referenced by **token name** in the database (`categories.color_token = 'cat-images'`), never by hex. Custom categories pick from the token list.

## 4. Component rules

1. Build on shadcn/ui primitives in `src/components/ui/`; their shadcn color names (`bg-primary`, `text-muted-foreground`) are aliased to our tokens in `theme.css`.
2. Variants (`variant="danger"`) map to token utilities inside the component — never inline styles.
3. Icons: Lucide, 16 px default (`size-4`), 14 px in dense rows; color via `text-*` token.
4. Every screen ships empty, loading, error and success states using the shared `EmptyState`, `LoadingState`, `ErrorState`, `Toast`.
5. Destructive actions: `ConfirmDialog` with `danger` button; bulk destructive actions require typing a count or "confirm".
6. Paths are shown with `PathLabel` (middle truncation, monospace off, reveal-in-Finder button).

## 5. Layout

```
┌──────────────┬───────────────────────────────────────────────┐
│ ● ● ●        │  Page title          [search]  ● Active  [+]  │  ← header (52 px)
│              ├───────────────────────────────────────────────┤
│ Overview     │                                               │
│ Folders      │                                               │
│ Rules        │              content (p-page)                 │
│ Preview      │                                               │
│ Activity     │                                               │
│ Duplicates   │                                               │
│ Scheduler    │                                               │
│ Categories   │                                               │
│ Settings     │                                               │
│              │                                               │
│ ● Monitoring │                                               │
│ Licensed ✓   │                                               │
└──────────────┴───────────────────────────────────────────────┘
  220 px sidebar (bg-sidebar), transparent titlebar with traffic-light inset
```

## 6. Enforcement — `scripts/check-tokens.mjs` (P1-T03)

Fails CI if any file under `src/` other than `src/styles/theme.css` contains:

- color literals: `#rgb`, `#rrggbb`, `rgb(`, `rgba(`, `hsl(`, `oklch(`, named colors in style props;
- Tailwind palette classes: `(bg|text|border|ring|fill|stroke)-(red|blue|gray|slate|zinc|…)-\d+`;
- arbitrary values: `-\[` (e.g. `p-[13px]`, `bg-[#eee]`);
- inline `style={{ color|background|fontSize|… }}` (except CSS variables for dynamic widths like progress bars).

It also verifies (UI-THEME-05) that the `[data-theme="dark"]` block and the system-dark media block in `theme.css` define identical values.

## 7. Accessibility

- Text tokens meet WCAG AA on their intended surfaces in both modes (verified in Phase 8 audit).
- Focus ring via `:focus-visible` in `theme.css`; never removed.
- `prefers-reduced-motion` zeroes durations.
- Status is never conveyed by color alone (dot + label).

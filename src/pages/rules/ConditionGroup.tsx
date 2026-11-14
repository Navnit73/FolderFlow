import { FolderPlus, Plus, Trash2, Ungroup } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { ConditionNode } from "@/services/rules";
import { ConditionRow, defaultConditionForKind } from "./ConditionRow";

const MAX_CONDITION_DEPTH = 5;

const selectClass =
  "h-control-sm rounded-md border border-border-strong bg-surface px-2 text-xs font-medium text-fg focus-visible:border-border-focus";

export function defaultLeafNode(): ConditionNode {
  return {
    type: "leaf",
    condition: defaultConditionForKind("text"),
  };
}

export function defaultSubgroupNode(): ConditionNode {
  return {
    type: "all",
    children: [defaultLeafNode()],
  };
}

interface ConditionGroupProps {
  node: ConditionNode;
  depth?: number;
  onChange: (next: ConditionNode) => void;
  onUngroup?: (liftedChildren: ConditionNode[]) => void;
  onRemoveGroup?: () => void;
}

export function ConditionGroup({
  node,
  depth = 1,
  onChange,
  onUngroup,
  onRemoveGroup,
}: ConditionGroupProps) {
  if (node.type === "leaf") {
    return (
      <ConditionRow
        condition={node.condition}
        canRemove={false}
        onChange={(nextCond) => onChange({ type: "leaf", condition: nextCond })}
      />
    );
  }

  const children: ConditionNode[] = node.type === "not" ? [node.child] : node.children;

  const setGroupMode = (mode: "all" | "any" | "not") => {
    if (mode === "not") {
      const first = children[0] ?? defaultLeafNode();
      onChange({ type: "not", child: first });
    } else {
      onChange({
        type: mode,
        children: children.length > 0 ? children : [defaultLeafNode()],
      });
    }
  };

  const setChildren = (nextChildren: ConditionNode[]) => {
    if (node.type === "not") {
      onChange({ type: "not", child: nextChildren[0] ?? defaultLeafNode() });
    } else {
      onChange({ type: node.type, children: nextChildren });
    }
  };

  const updateChild = (idx: number, nextChild: ConditionNode) => {
    const next = [...children];
    next[idx] = nextChild;
    setChildren(next);
  };

  const removeChild = (idx: number) => {
    if (children.length <= 1) return;
    setChildren(children.filter((_, i) => i !== idx));
  };

  const moveChild = (idx: number, direction: -1 | 1) => {
    const target = idx + direction;
    if (target < 0 || target >= children.length) return;
    const next = [...children];
    const a = next[idx];
    const b = next[target];
    if (!a || !b) return;
    next[idx] = b;
    next[target] = a;
    setChildren(next);
  };

  const ungroupChild = (idx: number, lifted: ConditionNode[]) => {
    const next = [...children.slice(0, idx), ...lifted, ...children.slice(idx + 1)];
    setChildren(next);
  };

  const canAddChildren = node.type !== "not";
  const canNestDeeper = depth < MAX_CONDITION_DEPTH;

  return (
    <div className="space-y-2.5 rounded-lg border border-border bg-subtle p-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <span className="text-xs font-medium text-fg-muted">Match</span>
          <select
            aria-label="Group match mode"
            value={node.type}
            onChange={(e) => setGroupMode(e.target.value as "all" | "any" | "not")}
            className={selectClass}
          >
            <option value="all">ALL of the following (AND)</option>
            <option value="any">ANY of the following (OR)</option>
            <option value="not">NOT (negate child)</option>
          </select>
          <span className="text-2xs text-fg-subtle">Depth {depth}/5</span>
        </div>

        <div className="flex flex-wrap items-center gap-1.5">
          {canAddChildren && (
            <Button
              size="sm"
              onClick={() => setChildren([...children, defaultLeafNode()])}
            >
              <Plus aria-hidden />
              Add condition
            </Button>
          )}
          {canAddChildren && (
            <Button
              size="sm"
              disabled={!canNestDeeper}
              onClick={() => setChildren([...children, defaultSubgroupNode()])}
            >
              <FolderPlus aria-hidden />
              Add subgroup
            </Button>
          )}
          {depth > 1 && onUngroup && (
            <Button
              size="sm"
              variant="ghost"
              aria-label="Ungroup"
              onClick={() => onUngroup(children)}
            >
              <Ungroup aria-hidden />
              Ungroup
            </Button>
          )}
          {depth > 1 && onRemoveGroup && (
            <Button
              size="sm"
              variant="ghost"
              aria-label="Remove subgroup"
              onClick={onRemoveGroup}
            >
              <Trash2 aria-hidden />
            </Button>
          )}
        </div>
      </div>

      <div className="space-y-2">
        {children.map((child, idx) =>
          child.type === "leaf" ? (
            <ConditionRow
              key={idx}
              condition={child.condition}
              canMoveUp={idx > 0}
              canMoveDown={idx < children.length - 1}
              canRemove={children.length > 1}
              onChange={(nextCond) =>
                updateChild(idx, { type: "leaf", condition: nextCond })
              }
              onMoveUp={() => moveChild(idx, -1)}
              onMoveDown={() => moveChild(idx, 1)}
              onRemove={() => removeChild(idx)}
            />
          ) : (
            <ConditionGroup
              key={idx}
              node={child}
              depth={depth + 1}
              onChange={(nextSub) => updateChild(idx, nextSub)}
              onUngroup={
                node.type !== "not"
                  ? (lifted) => ungroupChild(idx, lifted)
                  : undefined
              }
              onRemoveGroup={
                children.length > 1 ? () => removeChild(idx) : undefined
              }
            />
          ),
        )}
      </div>
    </div>
  );
}

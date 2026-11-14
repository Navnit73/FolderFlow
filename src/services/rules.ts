import { commands } from "@/bindings";
import type {
  Action_Serialize,
  ConditionNode_Serialize,
  Condition_Serialize,
  DateConditionOp,
  DateField,
  DatePlacement,
  DateSource,
  ExecutionMode,
  LocationOp_Serialize,
  RuleDraft_Deserialize,
  RuleDraft_Serialize,
  RuleOrigin,
  RuleTemplate_Serialize,
  RuleTestMatchItem,
  RuleTestReport,
  Rule_Serialize,
  SizeOp,
  SizeUnit,
  TemplatePreviewInput_Deserialize,
  TemplatePreviewOutput_Serialize,
  TextFieldKind,
  TextOp,
  ValidationIssue,
} from "@/bindings";
import { unwrap } from "./result";

export type Rule = Rule_Serialize;
export type RuleDraft = RuleDraft_Serialize;
export type RuleDraftInput = RuleDraft_Deserialize;
export type RuleTemplate = RuleTemplate_Serialize;
export type ConditionNode = ConditionNode_Serialize;
export type Condition = Condition_Serialize;
export type Action = Action_Serialize;
export type LocationOp = LocationOp_Serialize;
export type TemplatePreviewInput = TemplatePreviewInput_Deserialize;
export type TemplatePreviewOutput = TemplatePreviewOutput_Serialize;
export type {
  DateConditionOp,
  DateField,
  DatePlacement,
  DateSource,
  ExecutionMode,
  RuleOrigin,
  RuleTestMatchItem,
  RuleTestReport,
  SizeOp,
  SizeUnit,
  TextFieldKind,
  TextOp,
  ValidationIssue,
};

export const rulesService = {
  list: (): Promise<Rule[]> => unwrap(commands.rulesList()),
  get: (id: string): Promise<Rule> => unwrap(commands.rulesGet(id)),
  create: (draft: RuleDraftInput): Promise<Rule> => unwrap(commands.rulesCreate(draft)),
  createFromPreset: (presetId: string): Promise<Rule> => unwrap(commands.rulesCreateFromPreset(presetId)),
  update: (id: string, draft: RuleDraftInput): Promise<Rule> => unwrap(commands.rulesUpdate(id, draft)),
  remove: (id: string): Promise<null> => unwrap(commands.rulesDelete(id)),
  reorder: (orderedIds: string[]): Promise<Rule[]> => unwrap(commands.rulesReorder(orderedIds)),
  setEnabled: (id: string, enabled: boolean): Promise<Rule> => unwrap(commands.rulesSetEnabled(id, enabled)),
  duplicate: (id: string): Promise<Rule> => unwrap(commands.rulesDuplicate(id)),
  validate: (draft: RuleDraftInput, currentRuleId: string | null = null): Promise<ValidationIssue[]> =>
    unwrap(commands.rulesValidate(draft, currentRuleId)),
  presets: (): Promise<RuleTemplate[]> => unwrap(commands.rulesPresets()),
  templatePreview: (input: TemplatePreviewInput): Promise<TemplatePreviewOutput> =>
    unwrap(commands.templatePreview(input)),
  testFolder: (draft: RuleDraftInput, folderId: string): Promise<RuleTestReport> =>
    unwrap(commands.rulesTestFolder(draft, folderId)),
  pickDestinationFolder: (): Promise<string | null> => unwrap(commands.rulesPickDestinationFolder()),
};


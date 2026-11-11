// Typed wrappers over Tauri commands. One function per command (Gate 3).
import { invoke } from "@tauri-apps/api/core";
import type { Item } from "./bindings/Item";
import type { ItemId } from "./bindings/ItemId";
import type { ItemKind } from "./bindings/ItemKind";
import type { Overview } from "./bindings/Overview";
import type { ActionPlan } from "./bindings/ActionPlan";
import type { ActionRecord } from "./bindings/ActionRecord";
import type { ActionRequest } from "./bindings/ActionRequest";
import type { Fix } from "./bindings/Fix";
import type { StorageReport } from "./bindings/StorageReport";
import type { InstallPreview } from "./bindings/InstallPreview";
import type { InstallTarget } from "./bindings/InstallTarget";
import type { Board } from "./bindings/Board";
import type { MarketList } from "./bindings/MarketList";
import type { MarketFetched } from "./bindings/MarketFetched";
import type { Updates } from "./bindings/Updates";
import type { RuleTarget } from "./bindings/RuleTarget";
import type { Session } from "./bindings/Session";
import type { Secret } from "./bindings/Secret";
import type { SearchHit } from "./bindings/SearchHit";
import type { SettingsData } from "./bindings/SettingsData";
import type { CustomAgent } from "./bindings/CustomAgent";
import type { CliStatus } from "./bindings/CliStatus";

export const ipc = {
  overview: () => invoke<Overview>("overview"),
  /** macOS Reduce transparency (WebKit has no media query for it). */
  reduceTransparency: () => invoke<boolean>("reduce_transparency"),
  scanStart: () => invoke<void>("scan_start"),
  itemsList: (kind: ItemKind) => invoke<Item[]>("items_list", { kind }),
  itemDocument: (id: ItemId) => invoke<string>("item_document", { id }),
  memoryRuleTargets: () => invoke<RuleTarget[]>("memory_rule_targets"),
  storageGet: () => invoke<StorageReport>("storage_get"),
  storageRefresh: () => invoke<StorageReport>("storage_refresh"),
  secretsList: () => invoke<Secret[]>("secrets_list"),
  secretIgnore: (id: string, on: boolean) => invoke<void>("secret_ignore", { id, on }),
  secretReveal: (id: string) => invoke<string>("secret_reveal", { id }),
  sessionsList: () => invoke<Session[]>("sessions_list"),
  sessionsSearch: (query: string) => invoke<SearchHit[]>("sessions_search", { query }),
  sessionResume: (id: string) => invoke<string>("session_resume", { id }),
  sessionOpenProject: (id: string) => invoke<void>("session_open_project", { id }),
  actionPlan: (request: ActionRequest) => invoke<ActionPlan>("action_plan", { request }),
  actionApply: (planId: string) => invoke<ActionRecord>("action_apply", { planId }),
  actionUndo: (id: string) => invoke<ActionRecord>("action_undo", { id }),
  historyList: () => invoke<ActionRecord[]>("history_list"),
  fixesList: () => invoke<Fix[]>("fixes_list"),
  fixDismiss: (id: string) => invoke<void>("fix_dismiss", { id }),
  /** Gone hooks only: hides them from every list. No files change. */
  skillInspect: (path: string) => invoke<InstallPreview>("skill_inspect", { path }),
  skillTargets: (id: ItemId) => invoke<InstallTarget[]>("skill_targets", { id }),
  revealInFinder: (path: string) => invoke<void>("reveal_in_finder", { path }),
  settingsGet: () => invoke<SettingsData>("settings_get"),
  agentSetOn: (agent: string, on: boolean) => invoke<void>("agent_set_on", { agent, on }),
  customAgentSave: (a: { key: string | null; name: string; skillsDir: string; projectDir: string | null }) => invoke<CustomAgent>("custom_agent_save", a),
  customAgentDelete: (key: string) => invoke<void>("custom_agent_delete", { key }),
  /** Folder picker in the core; null on Cancel. */
  pickFolder: () => invoke<string | null>("pick_folder"),
  cliStatus: () => invoke<CliStatus>("cli_status"),
  cliInstall: () => invoke<CliStatus>("cli_install"),
  // Network: Marketplace screen only.
  marketBoard: (board: Board) => invoke<MarketList>("market_board", { board }),
  marketSearch: (query: string) => invoke<MarketList>("market_search", { query }),
  marketUpdates: (check: boolean) => invoke<Updates>("market_updates", { check }),
  marketFetch: (source: string, skillId: string) => invoke<MarketFetched>("market_fetch", { source, skillId }),
  marketAvatar: (owner: string) => invoke<string>("market_avatar", { owner }),
  marketOpenPage: (source: string, skillId: string) => invoke<void>("market_open_page", { source, skillId }),
};

export type Ipc = typeof ipc;

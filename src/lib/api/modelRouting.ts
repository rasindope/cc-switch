import { invoke } from "@tauri-apps/api/core";

export interface ModelRouteRule {
  id: string;
  enabled: boolean;
  matchType: "exact" | "prefix";
  pattern: string;
  providerId: string;
}
export interface ModelRoutingConfig {
  version: 1;
  enabled: boolean;
  rules: ModelRouteRule[];
}
export interface RoutingPreview {
  matchedRuleId: string | null;
  providerId: string | null;
  catalog: {
    models: { slug: string; display_name: string }[];
    warnings: string[];
  };
}
export const modelRoutingApi = {
  get: () => invoke<ModelRoutingConfig>("get_codex_model_routing"),
  save: (config: ModelRoutingConfig) =>
    invoke<{ restartRequired: boolean }>("save_codex_model_routing", {
      config,
    }),
  preview: (config: ModelRoutingConfig, model: string) =>
    invoke<RoutingPreview>("preview_codex_model_routing", { config, model }),
};

use crate::{app_config::AppType, codex_config, proxy::model_routing::*, store::AppState};
use serde::Serialize;
use tauri::State;

fn current(state: &AppState) -> Result<Option<crate::provider::Provider>, String> {
    let id = crate::settings::get_effective_current_provider(&state.db, &AppType::Codex)
        .map_err(|e| e.to_string())?;
    id.map(|id| state.db.get_provider_by_id(&id, "codex"))
        .transpose()
        .map(|p| p.flatten())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_codex_model_routing(state: State<'_, AppState>) -> Result<ModelRoutingConfig, String> {
    ModelRoutingConfig::load(&state.db).map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutingPreview {
    matched_rule_id: Option<String>,
    provider_id: Option<String>,
    catalog: CatalogPreview,
}

#[tauri::command]
pub fn preview_codex_model_routing(
    state: State<'_, AppState>,
    config: ModelRoutingConfig,
    model: String,
) -> Result<RoutingPreview, String> {
    config.validate(&state.db).map_err(|e| e.to_string())?;
    let default = current(&state)?;
    let rule = config.matching_rule(&model);
    let mut catalog =
        catalog_preview(&state.db, &config, default.as_ref()).map_err(|e| e.to_string())?;
    if let Ok(text) = codex_config::read_codex_config_text() {
        if text
            .parse::<toml::Table>()
            .ok()
            .is_some_and(|doc| doc.contains_key("model_catalog_json"))
            && codex_config::resolve_cc_switch_catalog_path(
                &text,
                &codex_config::get_codex_config_dir(),
            )
            .is_none()
        {
            catalog
                .warnings
                .push("当前使用用户自建模型目录，CCS 不会覆盖它；请自行合并模型列表".into());
        }
    }
    Ok(RoutingPreview {
        matched_rule_id: rule.map(|r| r.id.clone()),
        provider_id: rule
            .map(|r| r.provider_id.clone())
            .or_else(|| default.map(|p| p.id)),
        catalog,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveResult {
    pub(crate) restart_required: bool,
}

#[tauri::command]
pub async fn save_codex_model_routing(
    state: State<'_, AppState>,
    config: ModelRoutingConfig,
) -> Result<SaveResult, String> {
    save_model_routing_config(state.inner(), config).await
}

pub(crate) async fn save_model_routing_config(
    state: &AppState,
    config: ModelRoutingConfig,
) -> Result<SaveResult, String> {
    let _guard = state.proxy_service.lock_switch_for_app("codex").await;
    config.validate(&state.db).map_err(|e| e.to_string())?;
    let default = current(&state)?;
    if config.enabled {
        if default.is_none()
            || default
                .as_ref()
                .is_some_and(crate::proxy::providers::is_codex_official_provider)
        {
            return Err("请先选择 Codex 第三方供应商作为默认供应商".into());
        }
        let preview =
            catalog_preview(&state.db, &config, default.as_ref()).map_err(|e| e.to_string())?;
        if preview.models.is_empty() {
            return Err("模型目录为空，请先配置供应商模型".into());
        }
    }
    ModelRoutingConfig::load(&state.db).map_err(|e| e.to_string())?;
    let catalog_snapshot =
        codex_config::CodexModelCatalogFileSnapshot::capture().map_err(|e| e.to_string())?;
    let before = std::fs::read(codex_config::get_codex_model_catalog_path()).ok();
    let previous_text = codex_config::read_codex_config_text().map_err(|e| e.to_string())?;
    let apply = async {
        if state.proxy_service.get_takeover_status().await?.codex {
            let provider = default.ok_or("默认供应商不存在")?;
            state
                .proxy_service
                .sync_codex_live_with_model_routing(&provider, &config)
                .await?;
        }
        state
            .db
            .set_setting(
                SETTINGS_KEY,
                &serde_json::to_string(&config).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        Ok::<(), String>(())
    }
    .await;
    if let Err(error) = apply {
        catalog_snapshot
            .restore()
            .map_err(|e| format!("{error}; catalog rollback: {e}"))?;
        codex_config::write_codex_live_config_atomic(Some(&previous_text))
            .map_err(|e| format!("{error}; config rollback: {e}"))?;
        return Err(error);
    }
    let after = std::fs::read(codex_config::get_codex_model_catalog_path()).ok();
    let pointer = |text: &str| {
        text.parse::<toml::Table>()
            .ok()
            .and_then(|v| v.get("model_catalog_json").cloned())
    };
    let next_text = codex_config::read_codex_config_text().map_err(|e| e.to_string())?;
    Ok(SaveResult {
        restart_required: before != after || pointer(&previous_text) != pointer(&next_text),
    })
}

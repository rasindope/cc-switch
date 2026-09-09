//! Request-local Codex routing. Rules never change the selected provider card.
use crate::{database::Database, error::AppError, provider::Provider};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const SETTINGS_KEY: &str = "codex_model_routing_v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelRoutingConfig {
    pub version: u32,
    pub enabled: bool,
    pub rules: Vec<ModelRouteRule>,
}

impl Default for ModelRoutingConfig {
    fn default() -> Self {
        Self {
            version: 1,
            enabled: false,
            rules: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelRouteRule {
    pub id: String,
    pub enabled: bool,
    pub match_type: MatchType,
    pub pattern: String,
    pub provider_id: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum MatchType {
    Exact,
    Prefix,
}

impl ModelRoutingConfig {
    pub fn load(db: &Database) -> Result<Self, AppError> {
        let config: Self = match db.get_setting(SETTINGS_KEY)? {
            Some(value) => serde_json::from_str(&value).map_err(|e| {
                AppError::Message(format!("Codex model routing config is invalid: {e}"))
            })?,
            None => Self::default(),
        };
        config.validate_shape()?;
        Ok(config)
    }

    pub fn matching_rule(&self, model: &str) -> Option<&ModelRouteRule> {
        if !self.enabled {
            return None;
        }
        self.rules.iter().find(|rule| {
            rule.enabled
                && match rule.match_type {
                    MatchType::Exact => model == rule.pattern,
                    MatchType::Prefix => model.starts_with(&rule.pattern),
                }
        })
    }

    pub fn validate_shape(&self) -> Result<(), AppError> {
        if self.version != 1 {
            return Err(AppError::Message(
                "Unsupported model routing version".into(),
            ));
        }
        let mut ids = HashSet::new();
        let mut patterns = HashSet::new();
        for rule in &self.rules {
            if rule.id.trim().is_empty()
                || rule.pattern.trim().is_empty()
                || rule.pattern.trim() != rule.pattern
                || rule.provider_id.trim().is_empty()
                || !ids.insert(&rule.id)
                || !patterns.insert((rule.match_type, &rule.pattern))
            {
                return Err(AppError::Message("Model routing rules must have unique IDs and match patterns, and non-empty values".into()));
            }
        }
        Ok(())
    }

    pub fn validate(&self, db: &Database) -> Result<(), AppError> {
        self.validate_shape()?;
        for rule in &self.rules {
            target_provider(db, &rule.provider_id)?;
        }
        Ok(())
    }
}

pub fn target_provider(db: &Database, id: &str) -> Result<Provider, AppError> {
    let provider = db
        .get_provider_by_id(id, "codex")?
        .ok_or_else(|| AppError::Message(format!("Codex model route target is missing: {id}")))?;
    if super::providers::is_codex_official_provider(&provider) {
        return Err(AppError::Message(
            "Codex OAuth accounts cannot be model route targets".into(),
        ));
    }
    Ok(provider)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogPreview {
    pub models: Vec<serde_json::Value>,
    pub warnings: Vec<String>,
}

pub fn catalog_preview(
    db: &Database,
    config: &ModelRoutingConfig,
    default: Option<&Provider>,
) -> Result<CatalogPreview, AppError> {
    use serde_json::Value;
    config.validate(db)?;
    let mut providers = std::collections::BTreeMap::new();
    if let Some(provider) = default {
        providers.insert(provider.id.clone(), provider.clone());
    }
    for rule in config.rules.iter().filter(|r| r.enabled) {
        providers.insert(
            rule.provider_id.clone(),
            target_provider(db, &rule.provider_id)?,
        );
    }
    let mut models = std::collections::BTreeMap::new();
    let mut warnings = vec![];
    let mut gpt_models = None;
    for (id, provider) in providers {
        let text = provider
            .settings_config
            .get("config")
            .and_then(Value::as_str)
            .unwrap_or("");
        let profile = super::providers::resolve_codex_catalog_tool_profile(&provider);
        let catalog = crate::codex_config::codex_model_catalog_from_settings(
            &provider.settings_config,
            text,
            profile,
        )?;
        let entries = catalog
            .as_ref()
            .and_then(|c| c.get("models"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let entries = if entries.is_empty()
            && (config
                .rules
                .iter()
                .any(|r| r.enabled && r.provider_id == id && r.pattern.starts_with("gpt-"))
                || text.contains("model = \"gpt-"))
        {
            gpt_models
                .get_or_insert_with(crate::codex_config::load_routing_gpt_models)
                .clone()
        } else {
            entries
        };
        if entries.is_empty() {
            warnings.push(format!(
                "{}：缺少模型目录，请先在供应商中配置模型",
                provider.name
            ));
        }
        for entry in entries {
            let Some(slug) = entry.get("slug").and_then(Value::as_str) else {
                continue;
            };
            let owner = config
                .matching_rule(slug)
                .map(|r| r.provider_id.as_str())
                .or_else(|| default.map(|p| p.id.as_str()));
            if owner == Some(id.as_str()) {
                models.insert(slug.to_string(), entry);
            }
        }
    }
    Ok(CatalogPreview {
        models: models.into_values().collect(),
        warnings,
    })
}

pub fn attach_catalog(
    db: &Database,
    settings: &mut serde_json::Value,
    default: Option<&Provider>,
) -> Result<(), AppError> {
    let config = match settings.get("modelRoutingOverride") {
        Some(value) => {
            serde_json::from_value(value.clone()).map_err(|e| AppError::Message(e.to_string()))?
        }
        None => ModelRoutingConfig::load(db)?,
    };
    if !config.enabled {
        return Ok(());
    }
    if default.is_some_and(super::providers::is_codex_official_provider) {
        return Err(AppError::Message(
            "请先关闭模型路由，再切换官方 OAuth 供应商".into(),
        ));
    }
    let preview = catalog_preview(db, &config, default)?;
    if preview.models.is_empty() {
        return Err(AppError::Message(
            "模型路由目录为空，请先配置供应商模型".into(),
        ));
    }
    settings["modelRoutingCatalog"] =
        serde_json::json!({"models": preview.models, "_ccSwitchModelRouting": true});
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rule(id: &str, match_type: MatchType, pattern: &str) -> ModelRouteRule {
        ModelRouteRule {
            id: id.into(),
            enabled: true,
            match_type,
            pattern: pattern.into(),
            provider_id: "target".into(),
        }
    }
    #[test]
    fn ordered_matching_and_disabled_rules() {
        let mut config = ModelRoutingConfig {
            enabled: true,
            rules: vec![
                rule("1", MatchType::Exact, "gpt-a"),
                rule("2", MatchType::Prefix, "gpt-"),
            ],
            ..Default::default()
        };
        assert_eq!(config.matching_rule("gpt-a").unwrap().id, "1");
        assert_eq!(config.matching_rule("gpt-b").unwrap().id, "2");
        assert!(config.matching_rule("grok-a").is_none());
        config.rules[0].enabled = false;
        assert_eq!(config.matching_rule("gpt-a").unwrap().id, "2");
        config.enabled = false;
        assert!(config.matching_rule("gpt-a").is_none());
    }
    #[test]
    fn rejects_ambiguous_or_empty_configuration() {
        let mut config = ModelRoutingConfig {
            rules: vec![rule("1", MatchType::Prefix, "")],
            ..Default::default()
        };
        assert!(config.validate_shape().is_err());
        config.rules = vec![
            rule("1", MatchType::Prefix, "gpt-"),
            rule("2", MatchType::Prefix, "gpt-"),
        ];
        assert!(config.validate_shape().is_err());
        config.rules.pop();
        assert!(config.validate_shape().is_ok());
        config.version = 2;
        assert!(config.validate_shape().is_err());
    }

    fn provider(id: &str, url: &str, model: &str) -> Provider {
        let mut provider = Provider::with_id(
            id.into(),
            id.into(),
            serde_json::json!({
                "auth": {"OPENAI_API_KEY": format!("test-{id}")},
                "config": format!("model_provider = \"custom\"\nmodel = \"{model}\"\n[model_providers.custom]\nbase_url = \"{url}\"\nwire_api = \"responses\"\n"),
                "modelCatalog": {"models": [{"model": model, "contextWindow": 32000}]}
            }),
            None,
        );
        provider.meta = Some(crate::provider::ProviderMeta {
            api_format: Some("openai_responses".into()),
            ..Default::default()
        });
        provider
    }

    #[test]
    fn validates_targets_and_preserves_catalog_ownership() {
        let db = Database::memory().unwrap();
        let gpt = provider("gpt", "http://localhost", "gpt-test");
        let mut grok = provider("grok", "http://localhost", "grok-test");
        grok.settings_config["modelCatalog"]["models"] = serde_json::json!([
            {"model":"grok-test", "contextWindow":64000},
            {"model":"gpt-test", "contextWindow":128000}
        ]);
        db.save_provider("codex", &gpt).unwrap();
        db.save_provider("codex", &grok).unwrap();
        let config = ModelRoutingConfig {
            enabled: true,
            rules: vec![ModelRouteRule {
                provider_id: "grok".into(),
                ..rule("r", MatchType::Prefix, "grok-")
            }],
            ..Default::default()
        };
        let preview = catalog_preview(&db, &config, Some(&gpt)).unwrap();
        assert_eq!(preview.models.len(), 2);
        assert_eq!(preview.models[0]["slug"], "gpt-test");
        assert_eq!(preview.models[0]["context_window"], 32000);
        assert_eq!(preview.models[1]["context_window"], 64000);
        let mut official = provider("official", "https://api.openai.com", "gpt-test");
        official.category = Some("official".into());
        official.id = "codex-official".into();
        db.save_provider("codex", &official).unwrap();
        assert!(target_provider(&db, "codex-official").is_err());
        assert!(target_provider(&db, "missing").is_err());
        db.set_setting(SETTINGS_KEY, "broken").unwrap();
        assert!(ModelRoutingConfig::load(&db).is_err());
    }

    #[tokio::test]
    async fn routes_concurrent_requests_with_distinct_auth_without_switching() {
        use super::super::{
            failover_switch::FailoverSwitchManager,
            handler_context::RequestContext,
            provider_router::ProviderRouter,
            providers::{
                codex_chat_history::CodexChatHistoryStore, gemini_shadow::GeminiShadowStore,
            },
            server::ProxyState,
        };
        use axum::{
            http::{HeaderMap, StatusCode},
            response::IntoResponse,
            routing::post,
            Json, Router,
        };
        use serde_json::{json, Value};
        use std::sync::Arc;
        use tokio::sync::RwLock;
        let seen = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let mut servers = vec![];
        let db = Arc::new(Database::memory().unwrap());
        let mut rules = vec![];
        for name in ["gpt", "grok"] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let captured = seen.clone();
            let app = Router::new().route("/v1/responses", post(move |headers: HeaderMap, Json(body): Json<Value>| {
                let seen = captured.clone();
                async move {
                    seen.lock().await.push((name, headers.get("authorization").unwrap().to_str().unwrap().to_string(), body.clone()));
                    if body["model"] == "grok-pending" {
                        std::future::pending::<()>().await;
                    }
                    if body["model"] == "grok-fail" {
                        (StatusCode::BAD_REQUEST, Json(json!({"error":{"message":"unsupported model"}}))).into_response()
                    } else if body["stream"] == true {
                        let chunks = futures::stream::iter([
                            Ok::<_, std::io::Error>("event: response.output_text.delta\ndata: {\"delta\":\"hello\"}\n\n"),
                            Ok("event: response.completed\ndata: {\"response\":{\"status\":\"completed\"}}\n\n"),
                        ]);
                        ([("content-type", "text/event-stream")], axum::body::Body::from_stream(chunks)).into_response()
                    } else {
                        (StatusCode::OK, Json(json!({"id":"resp-test", "object":"response", "status":"completed", "model":body["model"], "output":[], "usage":{"input_tokens":2,"output_tokens":1}}))).into_response()
                    }
                }
            }));
            servers.push(tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            }));
            db.save_provider("codex", &provider(name, &url, &format!("{name}-test")))
                .unwrap();
            rules.push(ModelRouteRule {
                provider_id: name.into(),
                ..rule(name, MatchType::Prefix, &format!("{name}-"))
            });
        }
        db.set_current_provider("codex", "gpt").unwrap();
        let config = ModelRoutingConfig {
            enabled: true,
            rules,
            ..Default::default()
        };
        db.set_setting(SETTINGS_KEY, &serde_json::to_string(&config).unwrap())
            .unwrap();
        let state = ProxyState {
            db: db.clone(),
            config: Arc::new(RwLock::new(Default::default())),
            status: Arc::new(RwLock::new(Default::default())),
            start_time: Arc::new(RwLock::new(None)),
            current_providers: Arc::new(RwLock::new(Default::default())),
            provider_router: Arc::new(ProviderRouter::new(db.clone())),
            gemini_shadow: Arc::new(GeminiShadowStore::default()),
            codex_chat_history: Arc::new(CodexChatHistoryStore::default()),
            app_handle: None,
            failover_manager: Arc::new(FailoverSwitchManager::new(db.clone())),
        };
        let send = |model: &'static str| {
            let state = state.clone();
            async move {
                let body = json!({"model":model,"input":[{"role":"user","content":[{"type":"input_text","text":"test"},{"type":"input_image","image_url":"data:image/png;base64,aGVsbG8="}]}],"tools":[{"type":"function","name":"lookup","parameters":{"type":"object","properties":{}}}],"stream":model.ends_with("stream")});
                let ctx = RequestContext::new(
                    &state,
                    &body,
                    &HeaderMap::new(),
                    crate::app_config::AppType::Codex,
                    "Codex",
                    "codex",
                )
                .await
                .unwrap();
                assert_eq!(ctx.get_providers().len(), 1);
                ctx.create_forwarder(&state)
                    .forward_with_retry(
                        &crate::app_config::AppType::Codex,
                        http::Method::POST,
                        "/v1/responses",
                        body,
                        HeaderMap::new(),
                        Default::default(),
                        ctx.get_providers(),
                    )
                    .await
            }
        };
        let (gpt, grok) = tokio::join!(send("gpt-test"), send("grok-test"));
        assert!(gpt.is_ok(), "GPT request failed");
        assert!(grok.is_ok(), "Grok request failed");
        assert!(send("grok-fail").await.is_err());
        drop(gpt);
        drop(grok);
        let streaming = send("grok-stream")
            .await
            .unwrap_or_else(|_| panic!("stream failed"));
        assert!(streaming.response.is_sse());
        let bytes = streaming.response.bytes_with_limit(8192).await.unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("response.output_text.delta"));
        assert!(text.contains("response.completed"));
        drop(streaming.connection_guard);
        let pending = tokio::spawn(send("grok-pending"));
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while seen.lock().await.len() < 5 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        pending.abort();
        assert!(matches!(pending.await, Err(error) if error.is_cancelled()));
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while state.status.read().await.active_connections != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let calls = seen.lock().await;
        assert_eq!(calls.len(), 5);
        for (name, auth, body) in calls.iter() {
            assert_eq!(auth, &format!("Bearer test-{name}"));
            assert!(body["model"].as_str().unwrap().starts_with(name));
            assert_eq!(body["tools"][0]["name"], "lookup");
            assert_eq!(body["input"][0]["content"][1]["type"], "input_image");
        }
        assert_eq!(
            db.get_current_provider("codex").unwrap().as_deref(),
            Some("gpt")
        );
        assert_eq!(state.status.read().await.failover_count, 0);
        assert!(state.current_providers.read().await.is_empty());
        for server in servers {
            server.abort();
        }
    }
}

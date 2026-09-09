import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { providersApi } from "@/lib/api/providers";
import {
  modelRoutingApi,
  type ModelRoutingConfig,
  type ModelRouteRule,
  type RoutingPreview,
} from "@/lib/api/modelRouting";
import { resolveCodexOfficialIdentity } from "@/utils/providerCapabilities";
import type { Provider } from "@/types";

export function ModelRoutingPanel() {
  const [config, setConfig] = useState<ModelRoutingConfig | null>(null);
  const [providers, setProviders] = useState<Record<string, Provider>>({});
  const [model, setModel] = useState("");
  const [preview, setPreview] = useState<RoutingPreview | null>(null);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let active = true;
    Promise.all([modelRoutingApi.get(), providersApi.getAll("codex")])
      .then(([saved, all]) => {
        if (active) {
          setConfig(saved);
          setProviders(all);
        }
      })
      .catch((e) => {
        if (active) setError(String(e));
      });
    return () => {
      active = false;
    };
  }, []);
  const change = (next: ModelRoutingConfig) => {
    setConfig(next);
    setPreview(null);
    setMessage("");
    setError("");
  };
  const patch = (id: string, values: Partial<ModelRouteRule>) => {
    if (config)
      change({
        ...config,
        rules: config.rules.map((r) => (r.id === id ? { ...r, ...values } : r)),
      });
  };
  const move = (index: number, delta: number) => {
    if (!config) return;
    const rules = [...config.rules];
    [rules[index], rules[index + delta]] = [rules[index + delta], rules[index]];
    change({ ...config, rules });
  };
  const run = async (save: boolean) => {
    if (!config) return;
    setBusy(true);
    setError("");
    setMessage("");
    try {
      if (save) {
        const result = await modelRoutingApi.save(config);
        setMessage(
          result.restartRequired
            ? "已保存，模型目录已更新。请重启 Codex 加载列表。"
            : "已保存。规则从下一次请求生效；需开启 Codex 本地路由。",
        );
      } else {
        setPreview(await modelRoutingApi.preview(config, model));
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <section
      className="rounded-xl border p-5 space-y-4"
      aria-label="Codex 模型路由"
    >
      <h3 className="font-semibold">Codex 模型路由</h3>
      <p className="text-sm text-muted-foreground">
        按顺序匹配第一条规则，使用指定供应商。未命中时沿用默认供应商；命中后失败不会跨供应商转移。
      </p>
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
      {config && (
        <fieldset disabled={busy} className="space-y-4">
          <label className="flex items-center gap-3">
            <Switch
              aria-label="启用模型路由"
              checked={config.enabled}
              onCheckedChange={(enabled) => change({ ...config, enabled })}
            />
            启用模型路由
          </label>
          {config.rules.map((rule, index) => (
            <div
              key={rule.id}
              className="flex flex-wrap items-center gap-2 rounded border p-3"
              data-testid="model-route-row"
            >
              <Switch
                aria-label={`启用规则 ${index + 1}`}
                checked={rule.enabled}
                onCheckedChange={(enabled) => patch(rule.id, { enabled })}
              />
              <select
                aria-label={`匹配方式 ${index + 1}`}
                className="bg-background border rounded p-2"
                value={rule.matchType}
                onChange={(e) =>
                  patch(rule.id, {
                    matchType: e.target.value as ModelRouteRule["matchType"],
                  })
                }
              >
                <option value="prefix">前缀匹配</option>
                <option value="exact">精确匹配</option>
              </select>
              <Input
                aria-label={`模型匹配值 ${index + 1}`}
                className="w-44"
                placeholder="例如 grok-"
                value={rule.pattern}
                onChange={(e) => patch(rule.id, { pattern: e.target.value })}
              />
              <select
                aria-label={`目标供应商 ${index + 1}`}
                className="bg-background border rounded p-2 max-w-60"
                value={rule.providerId}
                onChange={(e) => patch(rule.id, { providerId: e.target.value })}
              >
                <option value="">选择供应商</option>
                {Object.values(providers)
                  .filter((p) => !resolveCodexOfficialIdentity("codex", p))
                  .map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.name}
                    </option>
                  ))}
              </select>
              <Button
                variant="outline"
                size="sm"
                disabled={index === 0 || busy}
                onClick={() => move(index, -1)}
                aria-label={`上移规则 ${index + 1}`}
              >
                ↑
              </Button>
              <Button
                variant="outline"
                size="sm"
                disabled={index === config.rules.length - 1 || busy}
                onClick={() => move(index, 1)}
                aria-label={`下移规则 ${index + 1}`}
              >
                ↓
              </Button>
              <Button
                variant="ghost"
                size="sm"
                onClick={() =>
                  change({
                    ...config,
                    rules: config.rules.filter((r) => r.id !== rule.id),
                  })
                }
              >
                删除
              </Button>
            </div>
          ))}
          <div className="flex gap-2">
            <Button
              variant="outline"
              onClick={() =>
                change({
                  ...config,
                  rules: [
                    ...config.rules,
                    {
                      id: crypto.randomUUID(),
                      enabled: true,
                      matchType: "prefix",
                      pattern: "",
                      providerId: "",
                    },
                  ],
                })
              }
            >
              添加规则
            </Button>
            <Button onClick={() => void run(true)}>保存规则</Button>
          </div>
          <div className="flex gap-2">
            <Input
              aria-label="预览模型"
              value={model}
              onChange={(e) => {
                setModel(e.target.value);
                setPreview(null);
              }}
              placeholder="输入模型名检查路由"
            />
            <Button variant="outline" onClick={() => void run(false)}>
              预览路由与目录
            </Button>
          </div>
        </fieldset>
      )}
      {message && (
        <p role="status" className="text-sm">
          {message}
        </p>
      )}
      {preview && (
        <div className="text-sm space-y-2">
          <p>
            {preview.matchedRuleId ? "命中规则" : "默认路由"} →{" "}
            {preview.providerId
              ? (providers[preview.providerId]?.name ?? preview.providerId)
              : "未配置供应商"}
          </p>
          {preview.catalog.warnings.map((w) => (
            <p key={w} role="status">
              {w}
            </p>
          ))}
          <p>
            合并目录：
            {preview.catalog.models.map((m) => m.slug).join("、") || "暂无模型"}
          </p>
        </div>
      )}
    </section>
  );
}

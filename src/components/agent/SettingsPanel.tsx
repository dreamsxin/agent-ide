import { useState, useEffect, useCallback } from "react";
import { useAgentStore } from "../../stores/useAgentStore";
import type { ModelProvider, ProviderPreset } from "../../types/agent";

// ====== 提供商预设 ======
const PROVIDERS: ProviderPreset[] = [
  {
    id: "openai",
    label: "OpenAI",
    defaultEndpoint: "https://api.openai.com/v1",
    defaultModel: "gpt-4o",
    models: ["gpt-4o", "gpt-4o-mini", "gpt-4-turbo", "gpt-4", "gpt-3.5-turbo"],
  },
  {
    id: "anthropic",
    label: "Anthropic",
    defaultEndpoint: "https://api.anthropic.com/v1",
    defaultModel: "claude-3-opus-20240229",
    models: [
      "claude-3-opus-20240229",
      "claude-3-sonnet-20240229",
      "claude-3-haiku-20240307",
      "claude-3-5-sonnet-20241022",
    ],
  },
  {
    id: "azure",
    label: "Azure OpenAI",
    defaultEndpoint: "https://{resource}.openai.azure.com",
    defaultModel: "gpt-4",
    models: ["gpt-4", "gpt-4o", "gpt-35-turbo"],
  },
  {
    id: "custom",
    label: "Custom Provider",
    defaultEndpoint: "",
    defaultModel: "",
    models: [],
  },
];

// ====== SettingsPanel ======
export default function SettingsPanel() {
  const llmEndpoint = useAgentStore((s) => s.llmEndpoint);
  const llmModel = useAgentStore((s) => s.llmModel);
  const apiKeyMasked = useAgentStore((s) => s.apiKeyMasked);
  const llmConfigured = useAgentStore((s) => s.llmConfigured);
  const fetchLlmConfig = useAgentStore((s) => s.fetchLlmConfig);
  const updateLlmConfig = useAgentStore((s) => s.updateLlmConfig);

  const [provider, setProvider] = useState<ModelProvider>("openai");
  const [endpoint, setEndpoint] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [model, setModel] = useState("");
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<{ type: "ok" | "err"; text: string } | null>(null);

  // 初始化：从后端加载配置
  useEffect(() => {
    fetchLlmConfig();
  }, [fetchLlmConfig]);

  // 后端配置回来之后填充表单
  useEffect(() => {
    if (llmConfigured) {
      setEndpoint(llmEndpoint);
      setModel(llmModel);
      // 尝试匹配 provider
      const matched = PROVIDERS.find((p) => p.defaultEndpoint && llmEndpoint.startsWith(p.defaultEndpoint));
      setProvider(matched?.id ?? "custom");
    }
  }, [llmConfigured, llmEndpoint, llmModel]);

  // 切换 provider 时自动填默认值
  const handleProviderChange = useCallback(
    (p: ModelProvider) => {
      setProvider(p);
      const preset = PROVIDERS.find((pr) => pr.id === p);
      if (preset) {
        setEndpoint(preset.defaultEndpoint);
        setModel(preset.defaultModel);
      }
      // 不清除 apiKey
    },
    []
  );

  // 保存
  const handleSave = useCallback(async () => {
    if (!endpoint.trim() || !apiKey.trim() || !model.trim()) {
      setMessage({ type: "err", text: "All fields are required" });
      return;
    }
    setSaving(true);
    setMessage(null);
    try {
      await updateLlmConfig(endpoint.trim(), apiKey.trim(), model.trim());
      setMessage({ type: "ok", text: "Saved successfully" });
      setApiKey(""); // 保存后清空输入框中的 key
    } catch (e) {
      setMessage({ type: "err", text: `Save failed: ${e}` });
    } finally {
      setSaving(false);
    }
  }, [endpoint, apiKey, model, updateLlmConfig]);

  const preset = PROVIDERS.find((p) => p.id === provider);

  return (
    <div className="p-3 text-xs overflow-auto h-full">
      <div className="text-surface-muted mb-3 font-semibold tracking-wide">
        Model Configuration
      </div>

      {/* 状态条 */}
      {llmConfigured && (
        <div className="mb-3 px-2 py-1.5 rounded bg-accent-green/10 border border-accent-green/30 text-accent-green text-[11px]">
          Connected: {llmEndpoint} · {llmModel}
          {apiKeyMasked && <span className="text-surface-muted ml-1">({apiKeyMasked})</span>}
        </div>
      )}

      {/* Provider 下拉 */}
      <label className="block text-surface-muted mb-1">Provider</label>
      <select
        value={provider}
        onChange={(e) => handleProviderChange(e.target.value as ModelProvider)}
        className="w-full mb-3 px-2 py-1.5 rounded bg-surface-base border border-surface-border text-surface-text text-xs outline-none focus:border-accent-blue"
      >
        {PROVIDERS.map((p) => (
          <option key={p.id} value={p.id}>
            {p.label}
          </option>
        ))}
      </select>

      {/* Endpoint */}
      <label className="block text-surface-muted mb-1">API Endpoint (Base URL)</label>
      <input
        type="text"
        value={endpoint}
        onChange={(e) => setEndpoint(e.target.value)}
        placeholder="https://api.openai.com/v1"
        className="w-full mb-3 px-2 py-1.5 rounded bg-surface-base border border-surface-border text-surface-text text-xs outline-none focus:border-accent-blue font-mono"
      />

      {/* API Key */}
      <label className="block text-surface-muted mb-1">
        API Key {apiKeyMasked && <span className="text-[10px]">(saved: {apiKeyMasked})</span>}
      </label>
      <input
        type="password"
        value={apiKey}
        onChange={(e) => setApiKey(e.target.value)}
        placeholder={apiKeyMasked ? "Enter to overwrite..." : "sk-..."}
        className="w-full mb-3 px-2 py-1.5 rounded bg-surface-base border border-surface-border text-surface-text text-xs outline-none focus:border-accent-blue font-mono"
      />

      {/* Model */}
      <label className="block text-surface-muted mb-1">Model</label>
      {preset && preset.models.length > 0 ? (
        <>
          <select
            value={model}
            onChange={(e) => setModel(e.target.value)}
            className="w-full mb-1 px-2 py-1.5 rounded bg-surface-base border border-surface-border text-surface-text text-xs outline-none focus:border-accent-blue"
          >
            <option value="">-- Select --</option>
            {preset.models.map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
          </select>
          <div className="flex gap-1 mb-3">
            <span className="text-[10px] text-surface-muted">or custom:</span>
          </div>
        </>
      ) : null}
      <input
        type="text"
        value={model}
        onChange={(e) => setModel(e.target.value)}
        placeholder="e.g. gpt-4o, claude-3-opus-20240229"
        className="w-full mb-3 px-2 py-1.5 rounded bg-surface-base border border-surface-border text-surface-text text-xs outline-none focus:border-accent-blue font-mono"
      />

      {/* Save */}
      <button
        onClick={handleSave}
        disabled={saving}
        className="w-full py-1.5 rounded bg-accent-blue hover:bg-accent-blue/80 text-white text-xs font-medium disabled:opacity-50 transition-colors"
      >
        {saving ? "Saving..." : "Save Configuration"}
      </button>

      {/* Message */}
      {message && (
        <div
          className={`mt-2 px-2 py-1 rounded text-[11px] ${
            message.type === "ok"
              ? "bg-accent-green/10 border border-accent-green/30 text-accent-green"
              : "bg-diff-remove/10 border border-diff-remove/30 text-diff-remove"
          }`}
        >
          {message.text}
        </div>
      )}

      <div className="mt-4 pt-3 border-t border-surface-border">
        <div className="text-surface-muted text-[10px] leading-relaxed">
          Configuration is stored in the backend session. Set environment variables{" "}
          <code className="bg-surface-border/50 px-1 rounded">LLM_ENDPOINT</code>,{" "}
          <code className="bg-surface-border/50 px-1 rounded">LLM_API_KEY</code>,{" "}
          <code className="bg-surface-border/50 px-1 rounded">LLM_MODEL</code> for defaults.
        </div>
      </div>
    </div>
  );
}

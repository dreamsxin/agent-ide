import { useState, useEffect, useCallback } from "react";
import { useAgentStore } from "../../stores/useAgentStore";
import type { ModelProvider, ProviderPreset } from "../../types/agent";

// ====== 提供商预设 ======
const providerLabels: Record<string, string> = {
  openai: "OpenAI",
  anthropic: "Anthropic",
  azure: "Azure OpenAI",
  deepseek: "DeepSeek",
  custom: "Custom",
};

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
    id: "deepseek",
    label: "DeepSeek",
    defaultEndpoint: "https://api.deepseek.com",
    defaultModel: "deepseek-chat",
    models: ["deepseek-chat", "deepseek-v4-flash"],
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
  const testLlmConnection = useAgentStore((s) => s.testLlmConnection);

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

  // 测试连接
  const [testing, setTesting] = useState(false);
  const handleTestConnection = useCallback(async () => {
    setTesting(true);
    setMessage(null);
    try {
      // 如果表单里还有新 key（用户修改后未点 Save），先保存
      if (apiKey.trim()) {
        await updateLlmConfig(endpoint.trim(), apiKey.trim(), model.trim());
        setApiKey(""); // 保存后清空输入框
      }
      // 后端已有配置，直接测试
      if (!llmConfigured) {
        setMessage({ type: "err", text: "No config saved. Fill fields and click Save first." });
        return;
      }
      const result = await testLlmConnection();
      setMessage({ type: "ok", text: result });
    } catch (e) {
      setMessage({ type: "err", text: `Test failed: ${e}` });
    } finally {
      setTesting(false);
    }
  }, [endpoint, apiKey, model, llmConfigured, updateLlmConfig, testLlmConnection]);

  const preset = PROVIDERS.find((p) => p.id === provider);

  return (
    <div className="p-3 text-xs overflow-auto h-full">
      <div className="text-surface-muted mb-3 font-semibold tracking-wide">
        Model Configuration
      </div>

      {/* 当前配置状态卡 */}
      {llmConfigured ? (
        <div className="mb-4 rounded border border-accent-green/30 bg-accent-green/5 overflow-hidden">
          <div className="px-3 py-1.5 bg-accent-green/10 border-b border-accent-green/20 text-accent-green text-[11px] font-medium flex items-center gap-1.5">
            <span>●</span> LLM Service Configured
          </div>
          <div className="px-3 py-2 space-y-1 text-[11px]">
            <div className="flex justify-between">
              <span className="text-surface-muted">Provider</span>
              <span className="text-surface-text font-medium">{providerLabels[provider] ?? provider}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-surface-muted">Model</span>
              <span className="text-surface-text font-mono">{llmModel}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-surface-muted">Endpoint</span>
              <span className="text-surface-text font-mono text-[10px] truncate max-w-[160px]" title={llmEndpoint}>{new URL(llmEndpoint).hostname}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-surface-muted">API Key</span>
              <span className="text-surface-text font-mono">{apiKeyMasked || '****'}</span>
            </div>
          </div>
        </div>
      ) : (
        <div className="mb-4 px-3 py-2 rounded border border-surface-border bg-surface-border/10 text-surface-muted text-[11px]">
          No LLM service configured. Fill in the form below to connect an AI model.
        </div>
      )}

      {/* Provider 下拉 */}
      <label className="block text-surface-muted mb-1 text-[11px]">AI Provider</label>
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
      <label className="block text-surface-muted mb-1 text-[11px]">API Base URL</label>
      <input
        type="text"
        value={endpoint}
        onChange={(e) => setEndpoint(e.target.value)}
        placeholder="https://api.openai.com/v1"
        className="w-full mb-3 px-2 py-1.5 rounded bg-surface-base border border-surface-border text-surface-text text-xs outline-none focus:border-accent-blue font-mono"
      />

      {/* API Key */}
      <label className="block text-surface-muted mb-1 text-[11px]">
        Secret Key {apiKeyMasked && <span className="text-[10px] text-accent-green">(saved)</span>}
      </label>
      <input
        type="password"
        value={apiKey}
        onChange={(e) => setApiKey(e.target.value)}
        placeholder={apiKeyMasked ? "Enter to overwrite..." : "sk-..."}
        className="w-full mb-3 px-2 py-1.5 rounded bg-surface-base border border-surface-border text-surface-text text-xs outline-none focus:border-accent-blue font-mono"
      />

      {/* Model */}
      <label className="block text-surface-muted mb-1 text-[11px]">Model Name</label>
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

      {/* Test Connection */}
      <button
        onClick={handleTestConnection}
        disabled={testing}
        className="w-full mt-2 py-1.5 rounded border border-accent-purple/50 text-accent-purple text-xs font-medium hover:bg-accent-purple/10 disabled:opacity-50 transition-colors"
      >
        {testing ? "Testing..." : "⚡ Test Connection"}
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
          Tip: Set <code className="bg-surface-border/50 px-1 rounded">LLM_ENDPOINT</code>,{" "}
          <code className="bg-surface-border/50 px-1 rounded">LLM_API_KEY</code>,{" "}
          <code className="bg-surface-border/50 px-1 rounded">LLM_MODEL</code> env vars for default values.
        </div>
      </div>
    </div>
  );
}

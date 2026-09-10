import { useState, useEffect, useCallback, useRef } from "react";
import { Eye, EyeOff } from "lucide-react";
import { useAgentStore } from "../../stores/useAgentStore";
import { microsToUsdInput, spendCapStatus, usdToMicros } from "../../utils/money";
import McpPanel from "./McpPanel";
import type { ModelProvider, ProviderPreset, AgentPermissionPreset } from "../../types/agent";

type ToolCallMode = "text_protocol" | "native_tools";

// ====== 提供商预设 ======
const providerLabels: Record<string, string> = {
  openai: "OpenAI",
  anthropic: "Anthropic",
  azure: "Azure OpenAI",
  deepseek: "DeepSeek",
  custom: "Custom",
  local: "Local GGUF",
};

const PROVIDERS: ProviderPreset[] = [
  {
    id: "openai",
    label: "OpenAI",
    defaultEndpoint: "https://api.openai.com/v1",
    defaultModel: "gpt-4o",
    models: ["gpt-4o", "gpt-4o-mini", "gpt-4-turbo", "gpt-4", "gpt-3.5-turbo"],
    defaultMaxContextTokens: 128000,
    defaultReservedOutputTokens: 4096,
    defaultMaxOutputTokens: 4096,
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
    defaultMaxContextTokens: 200000,
    defaultReservedOutputTokens: 8192,
    defaultMaxOutputTokens: 8192,
  },
  {
    id: "azure",
    label: "Azure OpenAI",
    defaultEndpoint: "https://{resource}.openai.azure.com",
    defaultModel: "gpt-4",
    models: ["gpt-4", "gpt-4o", "gpt-35-turbo"],
    defaultMaxContextTokens: 128000,
    defaultReservedOutputTokens: 4096,
    defaultMaxOutputTokens: 4096,
  },
  {
    id: "deepseek",
    label: "DeepSeek",
    defaultEndpoint: "https://api.deepseek.com",
    defaultModel: "deepseek-chat",
    models: ["deepseek-chat", "deepseek-v4-flash"],
    defaultMaxContextTokens: 64000,
    defaultReservedOutputTokens: 4096,
    defaultMaxOutputTokens: 4096,
  },
  {
    id: "custom",
    label: "Custom Provider",
    defaultEndpoint: "",
    defaultModel: "",
    models: [],
  },
  {
    id: "local",
    label: "Local GGUF",
    defaultEndpoint: "local://model",
    defaultModel: "StarCoder",
    models: ["StarCoder", "CodeLlama", "DeepSeek Coder", "CodeGemma"],
    defaultMaxContextTokens: 4096,
    defaultReservedOutputTokens: 512,
    defaultMaxOutputTokens: 512,
    defaultToolCallMode: "text_protocol",
  },
];

// ====== SettingsPanel ======
export default function SettingsPanel() {
  const llmEndpoint = useAgentStore((s) => s.llmEndpoint);
  const llmModel = useAgentStore((s) => s.llmModel);
  const apiKeyMasked = useAgentStore((s) => s.apiKeyMasked);
  const llmConfigured = useAgentStore((s) => s.llmConfigured);
  const llmProfiles = useAgentStore((s) => s.llmProfiles);
  const activeProfileId = useAgentStore((s) => s.activeProfileId);
  const fetchLlmConfig = useAgentStore((s) => s.fetchLlmConfig);
  const saveLlmProfile = useAgentStore((s) => s.saveLlmProfile);
  const revealLlmApiKey = useAgentStore((s) => s.revealLlmApiKey);
  const deleteLlmProfile = useAgentStore((s) => s.deleteLlmProfile);
  const setActiveLlmProfile = useAgentStore((s) => s.setActiveLlmProfile);
  const testLlmConnection = useAgentStore((s) => s.testLlmConnection);
  const permissionPreset = useAgentStore((s) => s.permissionPreset);
  const permissions = useAgentStore((s) => s.permissions);
  const setPermissionPreset = useAgentStore((s) => s.setPermissionPreset);
  const togglePermission = useAgentStore((s) => s.togglePermission);

  const [profileId, setProfileId] = useState("");
  const [revealedKey, setRevealedKey] = useState<string | null>(null);
  const [revealError, setRevealError] = useState("");
  const [profileName, setProfileName] = useState("Default");
  const [provider, setProvider] = useState<ModelProvider>("openai");
  const [endpoint, setEndpoint] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [model, setModel] = useState("");
  const [maxContextTokens, setMaxContextTokens] = useState("");
  const [reservedOutputTokens, setReservedOutputTokens] = useState("");
  const [maxOutputTokens, setMaxOutputTokens] = useState("");
  const [maxRunTokens, setMaxRunTokens] = useState("");
  // 价格和金额上限在界面上用美元，存到后端是整数微美元（见 usdToMicros）
  const [promptPrice, setPromptPrice] = useState("");
  const [completionPrice, setCompletionPrice] = useState("");
  const [maxRunSpend, setMaxRunSpend] = useState("");
  const spendCapState = spendCapStatus(promptPrice, completionPrice, maxRunSpend);
  const [toolCallMode, setToolCallMode] = useState<ToolCallMode>("text_protocol");
  const [saving, setSaving] = useState(false);
  // 后端探测不到可读条目时会返回 "not configured"，那不算已保存
  const hasSavedKey = Boolean(apiKeyMasked) && apiKeyMasked !== "not configured";
  const [message, setMessage] = useState<{ type: "ok" | "err"; text: string } | null>(null);
  const messageRef = useRef<HTMLDivElement>(null);

  // 面板很长，触发保存/测试的按钮可能已经滚出视野，反馈不滚回来就等于没有反馈
  useEffect(() => {
    if (message) messageRef.current?.scrollIntoView({ block: "nearest" });
  }, [message]);


  // 初始化：从后端加载配置
  useEffect(() => {
    fetchLlmConfig();
  }, [fetchLlmConfig]);

  // 后端配置回来之后填充表单
  useEffect(() => {
    if (llmConfigured) {
      const active = llmProfiles.find((profile) => profile.id === activeProfileId) ?? llmProfiles[0];
      if (active) {
        setProfileId(active.id);
        setProfileName(active.name);
        setProvider(active.provider);
        setEndpoint(active.endpoint);
        setModel(active.model);
        setMaxContextTokens(numberToInput(active.maxContextTokens));
        setReservedOutputTokens(numberToInput(active.reservedOutputTokens));
        setMaxOutputTokens(numberToInput(active.maxOutputTokens));
        setMaxRunTokens(numberToInput(active.maxRunTokens));
        setPromptPrice(microsToUsdInput(active.promptMicrosPerMillion));
        setCompletionPrice(microsToUsdInput(active.completionMicrosPerMillion));
        setMaxRunSpend(microsToUsdInput(active.maxRunSpendMicros));
        setToolCallMode(active.toolCallMode ?? "text_protocol");
      } else {
        setEndpoint(llmEndpoint);
        setModel(llmModel);
        const matched = PROVIDERS.find((p) => p.defaultEndpoint && llmEndpoint.startsWith(p.defaultEndpoint));
        setProvider(matched?.id ?? "custom");
      }
      setEndpoint(llmEndpoint);
      setModel(llmModel);
    }
  }, [activeProfileId, llmConfigured, llmEndpoint, llmModel, llmProfiles]);

  // 切换 provider 时自动填默认值
  const handleProviderChange = useCallback(
    (p: ModelProvider) => {
      setProvider(p);
      const preset = PROVIDERS.find((pr) => pr.id === p);
      if (preset) {
        setEndpoint(preset.defaultEndpoint);
        setModel(preset.defaultModel);
        setMaxContextTokens(numberToInput(preset.defaultMaxContextTokens));
        setReservedOutputTokens(numberToInput(preset.defaultReservedOutputTokens));
        setMaxOutputTokens(numberToInput(preset.defaultMaxOutputTokens));
        setToolCallMode(preset.defaultToolCallMode ?? "text_protocol");
      }
      // 不清除 apiKey
    },
    []
  );

  // 保存
  /** 眼睛图标：显示时向后端取一次明文，隐藏时只清本地状态 */
  const handleToggleReveal = useCallback(async () => {
    setRevealError("");
    if (revealedKey !== null) {
      setRevealedKey(null);
      return;
    }
    try {
      setRevealedKey(await revealLlmApiKey(profileId || null));
    } catch (e) {
      setRevealError(`Cannot read stored key: ${e}`);
    }
  }, [profileId, revealLlmApiKey, revealedKey]);

  const handleSave = useCallback(async () => {
    if (!profileName.trim() || !model.trim() || (provider !== "local" && !endpoint.trim())) {
      setMessage({ type: "err", text: provider === "local" ? "Profile name and model are required" : "Profile name, endpoint, and model are required" });
      return;
    }
    // 判断依据是"是否已有可读密钥"，而不是"是否是新 profile"：
    // 引导出来的 profile 有 id 但密钥可能从未真正存进凭据存储。
    if (provider !== "local" && !hasSavedKey && !apiKey.trim()) {
      setMessage({ type: "err", text: "Secret key is required for a new profile" });
      return;
    }
    setSaving(true);
    setMessage(null);
    try {
      await saveLlmProfile({
        id: profileId || undefined,
        name: profileName.trim(),
        provider,
        endpoint: endpoint.trim(),
        apiKey: apiKey.trim() || undefined,
        model: model.trim(),
        maxContextTokens: inputToNumber(maxContextTokens),
        reservedOutputTokens: inputToNumber(reservedOutputTokens),
        maxOutputTokens: inputToNumber(maxOutputTokens),
        maxRunTokens: inputToNumber(maxRunTokens),
        promptMicrosPerMillion: usdToMicros(promptPrice),
        completionMicrosPerMillion: usdToMicros(completionPrice),
        maxRunSpendMicros: usdToMicros(maxRunSpend),
        toolCallMode,
        setActive: true,
      });
      setMessage({ type: "ok", text: "Saved successfully" });
      setApiKey(""); // 保存后清空输入框中的 key
      setRevealedKey(null); // 明文回显必须重新点一次才显示，避免展示过期值
    } catch (e) {
      setMessage({ type: "err", text: `Save failed: ${e}` });
    } finally {
      setSaving(false);
    }
  }, [apiKey, completionPrice, endpoint, maxContextTokens, maxOutputTokens, maxRunSpend, maxRunTokens, model, profileId, profileName, promptPrice, provider, reservedOutputTokens, saveLlmProfile, toolCallMode]);

  // 测试连接
  const [testing, setTesting] = useState(false);
  const handleTestConnection = useCallback(async () => {
    setTesting(true);
    setMessage(null);
    try {
      // 如果表单里还有新 key（用户修改后未点 Save），先保存
      if (apiKey.trim()) {
        await saveLlmProfile({
          id: profileId || undefined,
          name: profileName.trim(),
          provider,
          endpoint: endpoint.trim(),
          apiKey: apiKey.trim(),
          model: model.trim(),
          maxContextTokens: inputToNumber(maxContextTokens),
          reservedOutputTokens: inputToNumber(reservedOutputTokens),
          maxOutputTokens: inputToNumber(maxOutputTokens),
          maxRunTokens: inputToNumber(maxRunTokens),
          promptMicrosPerMillion: usdToMicros(promptPrice),
          completionMicrosPerMillion: usdToMicros(completionPrice),
          maxRunSpendMicros: usdToMicros(maxRunSpend),
          toolCallMode,
          setActive: true,
        });
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
  }, [apiKey, completionPrice, endpoint, llmConfigured, maxContextTokens, maxOutputTokens, maxRunSpend, maxRunTokens, model, profileId, profileName, promptPrice, provider, reservedOutputTokens, saveLlmProfile, testLlmConnection, toolCallMode]);

  const handleProfileSelect = useCallback((id: string) => {
    const profile = llmProfiles.find((item) => item.id === id);
    if (!profile) return;
    setProfileId(profile.id);
    setProfileName(profile.name);
    setProvider(profile.provider);
    setEndpoint(profile.endpoint);
    setModel(profile.model);
    setMaxContextTokens(numberToInput(profile.maxContextTokens));
    setReservedOutputTokens(numberToInput(profile.reservedOutputTokens));
    setMaxOutputTokens(numberToInput(profile.maxOutputTokens));
    setMaxRunTokens(numberToInput(profile.maxRunTokens));
    setPromptPrice(microsToUsdInput(profile.promptMicrosPerMillion));
    setCompletionPrice(microsToUsdInput(profile.completionMicrosPerMillion));
    setMaxRunSpend(microsToUsdInput(profile.maxRunSpendMicros));
    setToolCallMode(profile.toolCallMode ?? "text_protocol");
    setApiKey("");
  }, [llmProfiles]);

  const handleNewProfile = useCallback(() => {
    const preset = PROVIDERS[0];
    setProfileId("");
    setProfileName("New Profile");
    setProvider(preset.id);
    setEndpoint(preset.defaultEndpoint);
    setModel(preset.defaultModel);
    setMaxContextTokens("");
    setReservedOutputTokens("");
    setMaxOutputTokens("");
    // 上限和价格也要清掉：新 profile 通常是另一个模型，沿用上一个的价格会让
    // 金额上限按错误的单价执行，那比没有上限更糟
    setMaxRunTokens("");
    setPromptPrice("");
    setCompletionPrice("");
    setMaxRunSpend("");
    setToolCallMode(preset.defaultToolCallMode ?? "text_protocol");
    setApiKey("");
  }, []);

  const handleSetDefault = useCallback(async () => {
    if (!profileId) return;
    try {
      await setActiveLlmProfile(profileId);
      setMessage({ type: "ok", text: "Default profile updated" });
    } catch (e) {
      setMessage({ type: "err", text: `Set default failed: ${e}` });
    }
  }, [profileId, setActiveLlmProfile]);

  const handleDelete = useCallback(async () => {
    if (!profileId) return;
    try {
      await deleteLlmProfile(profileId);
      setMessage({ type: "ok", text: "Profile deleted" });
    } catch (e) {
      setMessage({ type: "err", text: `Delete failed: ${e}` });
    }
  }, [deleteLlmProfile, profileId]);

  const preset = PROVIDERS.find((p) => p.id === provider);

  return (
    <div className="p-3 text-xs overflow-auto h-full">
      <div className="text-surface-muted mb-3 font-semibold tracking-wide">
        Provider Profiles
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
            <div className="flex justify-between">
              <span className="text-surface-muted">Tools</span>
              <span className="text-surface-text font-mono text-[10px]">{toolCallMode}</span>
            </div>
          </div>
        </div>
      ) : (
        <div className="mb-4 px-3 py-2 rounded border border-surface-border bg-surface-border/10 text-surface-muted text-[11px]">
          No LLM service configured. Fill in the form below to connect an AI model.
        </div>
      )}

      <label className="block text-surface-muted mb-1 text-[11px]">Profile</label>
      <div className="mb-3 grid grid-cols-[minmax(0,1fr)_auto] gap-1">
        <select
          value={profileId}
          onChange={(e) => handleProfileSelect(e.target.value)}
          className="min-w-0 px-2 py-1.5 rounded bg-surface-base border border-surface-border text-surface-text text-xs outline-none focus:border-accent-blue"
        >
          <option value="">New profile</option>
          {llmProfiles.map((profile) => (
            <option key={profile.id} value={profile.id}>
              {profile.name}{profile.id === activeProfileId ? " (default)" : ""}
            </option>
          ))}
        </select>
        <button
          type="button"
          onClick={handleNewProfile}
          className="rounded border border-surface-border px-2 py-1 text-[11px] text-surface-muted hover:text-surface-text"
        >
          New
        </button>
      </div>

      <label className="block text-surface-muted mb-1 text-[11px]">Profile Name</label>
      <input
        type="text"
        value={profileName}
        onChange={(e) => setProfileName(e.target.value)}
        placeholder="Work OpenAI"
        className="w-full mb-3 px-2 py-1.5 rounded bg-surface-base border border-surface-border text-surface-text text-xs outline-none focus:border-accent-blue"
      />

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

      {provider === "local" && (
        <div className="mb-3 rounded border border-accent-blue/30 bg-accent-blue/5 p-2 text-[10px] leading-relaxed text-surface-muted">
          In-process inference was removed. Serve the model through an OpenAI-compatible
          endpoint — Ollama <span className="font-mono">http://localhost:11434/v1</span>,
          LM Studio <span className="font-mono">http://localhost:1234/v1</span>, or vLLM — and
          configure it above as a normal endpoint plus model name. A profile left on this
          provider is refused when a run starts, with the same instructions.
        </div>
      )}


      {/* API Key */}
      <label className="block text-surface-muted mb-1 text-[11px]">
        Secret Key {hasSavedKey && <span className="text-[10px] text-accent-green">(saved)</span>}
      </label>
      <input
        type="password"
        value={apiKey}
        onChange={(e) => setApiKey(e.target.value)}
        placeholder={hasSavedKey ? "Enter to overwrite..." : "sk-..."}
        className="w-full mb-1 px-2 py-1.5 rounded bg-surface-base border border-surface-border text-surface-text text-xs outline-none focus:border-accent-blue font-mono"
      />

      {/* 已保存的密钥回显：默认掩码，点眼睛取一次明文。
          apiKeyMasked 现在由后端实际探测凭据存储得出，所以这里显示
          "not configured" 就意味着真的没存上，而不是界面猜的。 */}
      <div className="mb-3 flex items-center gap-1.5 text-[10px]">
        <span className="text-surface-muted">Stored:</span>
        <code
          data-testid="settings-stored-key"
          className="min-w-0 flex-1 truncate rounded bg-surface-border/40 px-1 py-0.5 font-mono text-surface-text"
        >
          {revealedKey ?? apiKeyMasked ?? "not configured"}
        </code>
        {hasSavedKey && (
          <button
            type="button"
            onClick={handleToggleReveal}
            title={revealedKey ? "Hide secret key" : "Show secret key"}
            aria-label={revealedKey ? "Hide secret key" : "Show secret key"}
            className="flex-shrink-0 rounded border border-surface-border px-1 py-0.5 text-surface-muted hover:text-surface-text"
          >
            {revealedKey ? <EyeOff size={11} /> : <Eye size={11} />}
          </button>
        )}
      </div>
      {revealError && (
        <div className="mb-3 rounded border border-diff-remove/40 bg-diff-remove/10 px-2 py-1 text-[10px] text-diff-remove">
          {revealError}
        </div>
      )}


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

      <div className="mb-3 rounded border border-surface-border bg-surface-border/10 p-2">
        <div className="mb-2 text-[11px] font-semibold text-surface-muted">
          Context Budget Estimate
        </div>
        <div className="grid grid-cols-3 gap-2">
          <BudgetInput
            label="Max context"
            value={maxContextTokens}
            onChange={setMaxContextTokens}
            placeholder="128000"
          />
          <BudgetInput
            label="Reserved output"
            value={reservedOutputTokens}
            onChange={setReservedOutputTokens}
            placeholder="4096"
          />
          <BudgetInput
            label="Max output"
            value={maxOutputTokens}
            onChange={setMaxOutputTokens}
            placeholder="4096"
          />
          <BudgetInput
            label="Per-run cap"
            value={maxRunTokens}
            onChange={setMaxRunTokens}
            placeholder="no limit"
          />
        </div>
        <div className="mt-2 text-[10px] leading-relaxed text-surface-muted">
          Effective input estimate:{" "}
          <span className="font-mono text-surface-text">
            {formatTokenBudget(estimateInputTokens(maxContextTokens, reservedOutputTokens, maxOutputTokens))}
          </span>
          . This is model metadata for budgeting; current context modes still control compression strategy.
          Per-run cap stops a run once the provider-reported total tokens reach it; leave it empty for no limit.
        </div>
      </div>

      <div className="mb-3 rounded border border-surface-border bg-surface-border/10 p-2">
        <div className="mb-2 text-[11px] font-semibold text-surface-muted">
          Per-Run Spend Cap
        </div>
        <div className="grid grid-cols-3 gap-2">
          <BudgetInput
            label="Input $/M tokens"
            value={promptPrice}
            onChange={setPromptPrice}
            placeholder="0.28"
            step="0.000001"
          />
          <BudgetInput
            label="Output $/M tokens"
            value={completionPrice}
            onChange={setCompletionPrice}
            placeholder="0.42"
            step="0.000001"
          />
          <BudgetInput
            label="Spend cap $"
            value={maxRunSpend}
            onChange={setMaxRunSpend}
            placeholder="no limit"
            step="0.01"
          />
        </div>
        <div className="mt-2 text-[10px] leading-relaxed text-surface-muted">
          {spendCapState === "active" ? (
            <>
              Active: a run stops once its estimated cost reaches{" "}
              <span className="font-mono text-surface-text">${maxRunSpend}</span>, checked before the
              token cap.
            </>
          ) : spendCapState === "no_price" ? (
            <span className="text-amber-300">
              Not enforced: both prices are required. With only one, the estimate would undercount
              and the cap would be a false guarantee, so spend is recorded as "not computable"
              instead.
            </span>
          ) : (
            <>
              Optional. Enter both per-million prices and a cap to stop a run on cost rather than on
              token count — useful when a model is cheap in tokens but expensive in money. Prices are
              stored to the millionth of a dollar.
            </>
          )}
        </div>
      </div>

      <div className="mb-3 rounded border border-surface-border bg-surface-border/10 p-2">
        <div className="mb-2 text-[11px] font-semibold text-surface-muted">
          Tool Call Mode
        </div>
        <select
          value={toolCallMode}
          onChange={(event) => setToolCallMode(event.target.value as ToolCallMode)}
          className="w-full rounded border border-surface-border bg-surface-base px-2 py-1.5 text-xs text-surface-text outline-none focus:border-accent-blue"
        >
          <option value="text_protocol">Text protocol</option>
          <option value="native_tools">Provider-native tools</option>
        </select>
        <div className="mt-2 text-[10px] leading-relaxed text-surface-muted">
          Native tools is required for the Agent to read the workspace during a run
          (read file, search text, list files) and for MCP tools. Without it the Agent only sees
          the context bundle assembled when the run starts, and has to guess file contents it was
          not given. If an endpoint rejects the <span className="font-mono">tools</span> parameter,
          the request is retried without it and the run is flagged in the action log; pick text
          protocol to skip that failed attempt.
        </div>
      </div>

      {/* Save */}
      <button
        onClick={handleSave}
        disabled={saving}
        className="w-full py-1.5 rounded bg-accent-blue hover:bg-accent-blue/80 text-white text-xs font-medium disabled:opacity-50 transition-colors"
      >
        {saving ? "Saving..." : "Save Profile"}
      </button>

      {/* 反馈紧跟按钮。以前它渲染在 100 行 JSX 之后（Agent Permissions 和 Test
          Connection 下面），在侧边栏里点完 Save 根本看不到，像是没有任何反应。 */}
      {message && (
        <div
          ref={messageRef}
          role="status"
          aria-live="polite"
          className={`mt-2 px-2 py-1 rounded text-[11px] ${
            message.type === "ok"
              ? "bg-accent-green/10 border border-accent-green/30 text-accent-green"
              : "bg-diff-remove/10 border border-diff-remove/30 text-diff-remove"
          }`}
        >
          {message.text}
        </div>
      )}


      {/* ▸▸▸▸ Agent Permission Settings ▸▸▸▸ */}
      <div className="mt-4 pt-3 border-t border-surface-border">
        <div className="mb-2 text-[11px] font-semibold text-surface-muted tracking-wide">
          Agent Permissions
        </div>

        {/* Permission Preset */}
        <label className="block text-surface-muted mb-1 text-[11px]">Permission Preset</label>
        <div className="mb-2 grid grid-cols-3 gap-1">
          {(["read-only", "create-files", "run-commands"] as AgentPermissionPreset[]).map((preset) => (
            <button
              key={preset}
              onClick={() => setPermissionPreset(preset)}
              className={`rounded border px-2 py-1.5 text-[11px] font-medium transition-colors ${
                permissionPreset === preset
                  ? "border-accent-blue bg-accent-blue/10 text-accent-blue"
                  : "border-surface-border text-surface-muted hover:text-surface-text"
              }`}
            >
              {preset === "read-only"
                ? "\u{1F441} Read only"
                : preset === "create-files"
                ? "\u{1F4DD} Create files"
                : "\u{26A1} Run commands"}
            </button>
          ))}
        </div>
        <p className="mb-3 text-[10px] leading-relaxed text-surface-muted">
          {permissionPreset === "read-only"
            ? "The Agent can read the workspace and propose changes, nothing else."
            : permissionPreset === "create-files"
            ? "Also lets the Agent create new files. Changes still wait in the review area."
            : "Also lets the Agent run the project's own declared commands (tests, build)."}
        </p>

        {/* Granular Toggles */}
        <div className="mb-3 space-y-1.5 rounded border border-surface-border bg-surface-border/10 p-2">
          <PermissionToggle
            label="File Creation"
            desc="Allow Agent to create new files"
            checked={permissions.allowFileCreate}
            onChange={() => togglePermission("allowFileCreate")}
          />
          <PermissionToggle
            label="Command Execution"
            desc="Allow Agent to run the project's declared commands"
            checked={permissions.allowCommandRun}
            onChange={() => togglePermission("allowCommandRun")}
          />
        </div>
      </div>


      <div className="mt-2 grid grid-cols-2 gap-2">
        <button
          type="button"
          onClick={handleSetDefault}
          disabled={!profileId || profileId === activeProfileId}
          className="rounded border border-surface-border py-1.5 text-[11px] text-surface-muted hover:text-surface-text disabled:cursor-not-allowed disabled:opacity-40"
        >
          Set Default
        </button>
        <button
          type="button"
          onClick={handleDelete}
          disabled={!profileId || llmProfiles.length <= 1}
          className="rounded border border-diff-remove/40 py-1.5 text-[11px] text-diff-remove hover:bg-diff-remove/10 disabled:cursor-not-allowed disabled:opacity-40"
        >
          Delete
        </button>
      </div>

      {/* Test Connection */}
      <button
        onClick={handleTestConnection}
        disabled={testing}
        className="w-full mt-2 py-1.5 rounded border border-accent-purple/50 text-accent-purple text-xs font-medium hover:bg-accent-purple/10 disabled:opacity-50 transition-colors"
      >
        {testing ? "Testing..." : "⚡ Test Connection"}
      </button>

      {/* 反馈统一渲染在 Save Profile 按钮下方，见上。Test Connection 触发时靠
          messageRef 滚回视野，避免同一个 live region 出现两份。 */}


      <div className="mt-4 pt-3 border-t border-surface-border">
        <div className="text-surface-muted text-[10px] leading-relaxed">
          Tip: Set <code className="bg-surface-border/50 px-1 rounded">LLM_ENDPOINT</code>,{" "}
          <code className="bg-surface-border/50 px-1 rounded">LLM_API_KEY</code>,{" "}
          <code className="bg-surface-border/50 px-1 rounded">LLM_MODEL</code> env vars for default values.
        </div>
      </div>

      <McpPanel />
    </div>
  );
}

function BudgetInput({
  label,
  value,
  onChange,
  placeholder,
  step,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
  /** 金额字段要用小数步进，默认的 1 会让浏览器拒绝 "0.28" */
  step?: string;
}) {
  return (
    <label className="min-w-0">
      <span className="mb-1 block truncate text-[10px] text-surface-muted">{label}</span>
      <input
        type="number"
        min={0}
        step={step}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
        className="w-full rounded border border-surface-border bg-surface-base px-2 py-1 text-[11px] text-surface-text outline-none focus:border-accent-blue"
      />
    </label>
  );
}

function inputToNumber(value: string) {
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed > 0 ? Math.floor(parsed) : undefined;
}

function numberToInput(value?: number) {
  return value ? String(value) : "";
}

function estimateInputTokens(maxContext: string, reservedOutput: string, maxOutput: string) {
  const context = inputToNumber(maxContext);
  if (!context) return undefined;
  const reserved = inputToNumber(reservedOutput) ?? inputToNumber(maxOutput) ?? 4096;
  return Math.max(0, context - reserved - 512);
}

function formatTokenBudget(value?: number) {
  if (value === undefined) return "not set";
  return value.toLocaleString();
}

function PermissionToggle({
  label,
  desc,
  checked,
  onChange,
}: {
  label: string;
  desc: string;
  checked: boolean;
  onChange: () => void;
}) {
  return (
    <label className="flex items-center gap-2 cursor-pointer">
      <input
        type="checkbox"
        checked={checked}
        onChange={onChange}
        className="rounded border-surface-border"
      />
      <div className="min-w-0">
        <div className="text-[11px] text-surface-text">{label}</div>
        <div className="text-[10px] text-surface-muted">{desc}</div>
      </div>
    </label>
  );
}

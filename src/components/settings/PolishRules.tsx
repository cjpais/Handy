import React, { useState, useEffect } from "react";
import { Button } from "../ui/Button";
import { Input } from "../ui/Input";
import { Trash2, Edit, Plus, Eye, EyeOff } from "lucide-react";
import { useSettingsStore } from "../../stores/settingsStore";
import { PolishRule } from "../../lib/types";

interface PolishRuleFormData {
  name: string;
  api_url: string;
  api_key: string;
  model: string;
  prompt: string;
}

const PolishRules: React.FC = () => {
  const { getPolishRules, addPolishRule, updatePolishRule, deletePolishRule, togglePolishRule, getSetting, updateSetting } = useSettingsStore();
  
  const [rules, setRules] = useState<PolishRule[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [isFormVisible, setIsFormVisible] = useState(false);
  const [editingRule, setEditingRule] = useState<PolishRule | null>(null);
  const [formData, setFormData] = useState<PolishRuleFormData>({
    name: "",
    api_url: "",
    api_key: "",
    model: "",
    prompt: "",
  });
  const [showApiKeys, setShowApiKeys] = useState<Record<string, boolean>>({});
  const [errors, setErrors] = useState<Partial<PolishRuleFormData>>({});

  useEffect(() => {
    loadRules();
  }, []);

  const loadRules = async () => {
    try {
      setIsLoading(true);
      const polishRules = await getPolishRules();
      setRules(polishRules);
    } catch (error) {
      console.error("Failed to load polish rules:", error);
    } finally {
      setIsLoading(false);
    }
  };

  const validateForm = (): boolean => {
    const newErrors: Partial<PolishRuleFormData> = {};

    if (!formData.name.trim()) {
      newErrors.name = "规则名称不能为空";
    }

    if (!formData.api_url.trim()) {
      newErrors.api_url = "API URL 不能为空";
    } else {
      try {
        new URL(formData.api_url);
      } catch {
        newErrors.api_url = "请输入有效的 URL";
      }
    }

    if (!formData.api_key.trim()) {
      newErrors.api_key = "API Key 不能为空";
    }

    if (!formData.model.trim()) {
      newErrors.model = "模型名称不能为空";
    }

    if (!formData.prompt.trim()) {
      newErrors.prompt = "提示词不能为空";
    }

    setErrors(newErrors);
    return Object.keys(newErrors).length === 0;
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    
    if (!validateForm()) {
      return;
    }

    try {
      if (editingRule) {
        await updatePolishRule(
          editingRule.id,
          formData.name,
          formData.api_url,
          formData.api_key,
          formData.model,
          formData.prompt,
          editingRule.enabled
        );
      } else {
        await addPolishRule(
          formData.name,
          formData.api_url,
          formData.api_key,
          formData.model,
          formData.prompt
        );
      }
      
      await loadRules();
      handleCloseForm();
    } catch (error) {
      console.error("Failed to save polish rule:", error);
    }
  };

  const handleEdit = (rule: PolishRule) => {
    setEditingRule(rule);
    setFormData({
      name: rule.name,
      api_url: rule.api_url,
      api_key: rule.api_key,
      model: rule.model,
      prompt: rule.prompt,
    });
    setIsFormVisible(true);
  };

  const handleDelete = async (id: string) => {
    if (confirm("确定要删除这个润色规则吗？")) {
      try {
        await deletePolishRule(id);
        await loadRules();
      } catch (error) {
        console.error("Failed to delete polish rule:", error);
      }
    }
  };

  const handleToggle = async (id: string, enabled: boolean) => {
    try {
      await togglePolishRule(id, enabled);
      await loadRules();
    } catch (error) {
      console.error("Failed to toggle polish rule:", error);
    }
  };

  const handleCloseForm = () => {
    setIsFormVisible(false);
    setEditingRule(null);
    setFormData({
      name: "",
      api_url: "",
      api_key: "",
      model: "",
      prompt: "",
    });
    setErrors({});
  };

  const toggleApiKeyVisibility = (ruleId: string) => {
    setShowApiKeys(prev => ({
      ...prev,
      [ruleId]: !prev[ruleId]
    }));
  };

  const maskApiKey = (apiKey: string): string => {
    if (apiKey.length <= 8) return "*".repeat(apiKey.length);
    return apiKey.substring(0, 4) + "*".repeat(apiKey.length - 8) + apiKey.substring(apiKey.length - 4);
  };

  if (isLoading) {
    return (
      <div className="p-6">
        <div className="flex justify-center p-4">加载中...</div>
      </div>
    );
  }

  return (
    <div className="p-6 space-y-6">
      {/* Auto Polish Toggle */}
      <div className="border rounded-lg p-4 bg-gray-50">
        <div className="flex items-center justify-between">
          <div>
            <h4 className="text-md font-medium">自动润色</h4>
            <p className="text-sm text-gray-600 mt-1">
              在语音输入完成后自动应用润色规则
            </p>
          </div>
          <label className="flex items-center cursor-pointer">
            <input
              type="checkbox"
              checked={getSetting("auto_polish") || false}
              onChange={(e) => updateSetting("auto_polish", e.target.checked)}
              className="sr-only"
            />
            <div className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors ${
              getSetting("auto_polish") ? "bg-logo-primary" : "bg-gray-300"
            }`}>
              <span className={`inline-block h-4 w-4 transform rounded-full bg-white transition-transform ${
                getSetting("auto_polish") ? "translate-x-6" : "translate-x-1"
              }`} />
            </div>
          </label>
        </div>
      </div>

      <div className="flex justify-between items-center">
        <h3 className="text-lg font-medium">润色规则</h3>
        <Button onClick={() => {
          setEditingRule(null);
          setIsFormVisible(true);
        }}>
          <Plus className="w-4 h-4 mr-2" />
          添加规则
        </Button>
      </div>

      {isFormVisible && (
        <div className="border rounded-lg p-4 bg-gray-50">
          <h4 className="text-md font-medium mb-4">
            {editingRule ? "编辑润色规则" : "添加润色规则"}
          </h4>
          <form onSubmit={handleSubmit} className="space-y-4">
            <div>
              <label className="block text-sm font-medium mb-1">规则名称</label>
              <Input
                value={formData.name}
                onChange={(e) => setFormData({ ...formData, name: e.target.value })}
                placeholder="输入规则名称"
              />
              {errors.name && <p className="text-sm text-red-500 mt-1">{errors.name}</p>}
            </div>

            <div>
              <label className="block text-sm font-medium mb-1">API URL</label>
              <Input
                value={formData.api_url}
                onChange={(e) => setFormData({ ...formData, api_url: e.target.value })}
                placeholder="https://api.openai.com/v1/chat/completions"
              />
              {errors.api_url && <p className="text-sm text-red-500 mt-1">{errors.api_url}</p>}
            </div>

            <div>
              <label className="block text-sm font-medium mb-1">API Key</label>
              <Input
                type="password"
                value={formData.api_key}
                onChange={(e) => setFormData({ ...formData, api_key: e.target.value })}
                placeholder="输入 API Key"
              />
              {errors.api_key && <p className="text-sm text-red-500 mt-1">{errors.api_key}</p>}
            </div>

            <div>
              <label className="block text-sm font-medium mb-1">模型</label>
              <Input
                value={formData.model}
                onChange={(e) => setFormData({ ...formData, model: e.target.value })}
                placeholder="gpt-3.5-turbo"
              />
              {errors.model && <p className="text-sm text-red-500 mt-1">{errors.model}</p>}
            </div>

            <div>
              <label className="block text-sm font-medium mb-1">提示词</label>
              <textarea
                className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
                value={formData.prompt}
                onChange={(e) => setFormData({ ...formData, prompt: e.target.value })}
                placeholder="请润色以下文本，使其更加流畅和专业..."
                rows={4}
              />
              {errors.prompt && <p className="text-sm text-red-500 mt-1">{errors.prompt}</p>}
            </div>

            <div className="flex justify-end space-x-2">
              <Button type="button" onClick={handleCloseForm}>
                取消
              </Button>
              <Button type="submit">
                {editingRule ? "更新" : "添加"}
              </Button>
            </div>
          </form>
        </div>
      )}

      {rules.length === 0 ? (
        <div className="border rounded-lg p-6 text-center text-gray-500">
          暂无润色规则，点击上方按钮添加第一个规则
        </div>
      ) : (
        <div className="space-y-3">
          {rules.map((rule) => (
            <div key={rule.id} className="border rounded-lg p-4">
              <div className="flex items-center justify-between mb-3">
                <h4 className="text-base font-medium">{rule.name}</h4>
                <div className="flex items-center space-x-2">
                  <label className="flex items-center cursor-pointer">
                    <input
                      type="checkbox"
                      checked={rule.enabled}
                      onChange={(e) => handleToggle(rule.id, e.target.checked)}
                      className="sr-only"
                    />
                    <div className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors ${
                      rule.enabled ? 'bg-blue-600' : 'bg-gray-200'
                    }`}>
                      <span className={`inline-block h-4 w-4 transform rounded-full bg-white transition-transform ${
                        rule.enabled ? 'translate-x-6' : 'translate-x-1'
                      }`} />
                    </div>
                  </label>
                  <Button
                    size="sm"
                    onClick={() => handleEdit(rule)}
                  >
                    <Edit className="w-4 h-4" />
                  </Button>
                  <Button
                    size="sm"
                    onClick={() => handleDelete(rule.id)}
                  >
                    <Trash2 className="w-4 h-4" />
                  </Button>
                </div>
              </div>
              <div className="grid grid-cols-2 gap-4 text-sm">
                <div>
                  <span className="font-medium">API URL:</span>
                  <p className="text-gray-600 break-all">{rule.api_url}</p>
                </div>
                <div>
                  <span className="font-medium">模型:</span>
                  <p className="text-gray-600">{rule.model}</p>
                </div>
                <div className="col-span-2">
                  <div className="flex items-center justify-between">
                    <span className="font-medium">API Key:</span>
                    <Button
                      size="sm"
                      onClick={() => toggleApiKeyVisibility(rule.id)}
                    >
                      {showApiKeys[rule.id] ? (
                        <EyeOff className="w-4 h-4" />
                      ) : (
                        <Eye className="w-4 h-4" />
                      )}
                    </Button>
                  </div>
                  <p className="text-gray-600 font-mono text-xs break-all">
                    {showApiKeys[rule.id] ? rule.api_key : maskApiKey(rule.api_key)}
                  </p>
                </div>
                <div className="col-span-2">
                  <span className="font-medium">提示词:</span>
                  <p className="text-gray-600 mt-1 whitespace-pre-wrap">{rule.prompt}</p>
                </div>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
};

export default PolishRules;
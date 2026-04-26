# 离线本地翻译方案

> 目标：在转录完成后，将中文（或其他语言）自动翻译为英语，完全在本地运行，无需联网。

---

## 现状

当前 Whisper 系列模型内置"翻译为英语"功能（语音→英文直出），但：
- 非 Whisper 模型（SenseVoice、Parakeet 等）不支持翻译
- Whisper 翻译质量一般，不适合对质量要求高的场景
- DeepL 不开源、无离线版，仅有付费 API

---

## 方案一：Ollama 后处理（推荐，立即可用）

### 原理
利用现有「后处理 AI」功能，将转录文本发送给本地 Ollama 大模型进行翻译。

### 步骤

1. **安装 Ollama**
   ```bash
   # 官网下载：https://ollama.com
   ollama pull qwen2.5:7b   # 中英翻译质量好
   # 或更小的模型
   ollama pull qwen2.5:3b
   ```

2. **配置后处理**
   - 打开「高级」→「实验性功能」开启
   - 进入「后处理」页面
   - Provider 选择 `Custom`
   - Base URL 填写：`http://localhost:11434/v1`
   - 模型填写：`qwen2.5:7b`

3. **配置 Prompt**
   ```
   将以下内容翻译为英语，只输出翻译结果，不要解释：

   ${output}
   ```

### 优缺点
| | |
|---|---|
| ✅ 无需改代码，今天就能用 | ❌ 需要额外启动 Ollama 进程 |
| ✅ 翻译质量高（LLM 级别） | ❌ 内存占用较大（4～8GB） |
| ✅ 支持任意语言对 | ❌ 首次响应稍慢 |

---

## 方案二：原生 ONNX 翻译模型集成（待开发）

### 原理
将开源翻译模型导出为 ONNX 格式，使用项目已有的 `ort`（ONNX Runtime）加载推理，与 SenseVoice/Parakeet 集成方式相同，无需额外进程。

### 候选模型

| 模型 | 质量 | 大小 | 特点 |
|------|------|------|------|
| **Helsinki-NLP/opus-mt-zh-en** | ⭐⭐⭐ | ~300MB | 专用中英，最小 |
| **NLLB-200-distilled-600M** | ⭐⭐⭐⭐ | 600MB | Meta 出品，200 种语言 |
| **M2M100-418M** | ⭐⭐⭐⭐ | 418MB | Meta 出品，100 种语言 |
| **NLLB-200-1.3B** | ⭐⭐⭐⭐⭐ | 1.3GB | 质量最好，较大 |

### 推荐组合
- **速度优先**：`opus-mt-zh-en`（300MB，纯中英）
- **多语言**：`nllb-200-distilled-600M`（600MB，支持 200 种语言含中文）

### 技术路径

```
1. 模型导出
   Python: transformers + optimum 导出 ONNX
   opus-mt-zh-en → encoder.onnx + decoder.onnx + decoder_with_past.onnx

2. Rust 集成
   - 参考 src-tauri/src/managers/transcription.rs 的 ORT 推理模式
   - 新建 src-tauri/src/managers/translation.rs
   - 实现 TranslationManager（加载模型 / 推理 / tokenizer）

3. Tokenizer
   - 使用 tokenizers crate（HuggingFace 官方 Rust 实现）
   - opus-mt 使用 SentencePiece，需要 sentencepiece crate

4. 前端集成
   - 在转录完成后 pipeline 末尾加一步翻译
   - 或作为独立的后处理选项
```

### 模型导出脚本（备用）

```python
# export_opus_mt.py
from optimum.exporters.onnx import main_export

main_export(
    model_name_or_path="Helsinki-NLP/opus-mt-zh-en",
    output="./onnx/opus-mt-zh-en",
    task="seq2seq-lm",
    opset=14,
)
```

```python
# export_nllb.py
from optimum.exporters.onnx import main_export

main_export(
    model_name_or_path="facebook/nllb-200-distilled-600M",
    output="./onnx/nllb-200-600m",
    task="seq2seq-lm",
    opset=14,
)
```

### 开发工作量估算
- 模型导出 + 验证：1～2 天
- Rust TranslationManager 实现：3～5 天
- 前端 UI 集成（语言选择、开关）：1～2 天
- 测试调优：1～2 天
- **合计：约 2 周**

---

## 方案三：LibreTranslate 本地服务（备选）

```bash
pip install libretranslate
libretranslate --host 0.0.0.0 --port 5000
```

在后处理里填写 `http://localhost:5000`，使用其 REST API。
质量中等，适合不想跑大模型的场景。

---

## 实施建议

```
阶段 1（现在）：  Ollama + qwen2.5 后处理方案验证效果
阶段 2（1个月后）：评估是否需要原生集成
阶段 3（有需求时）：开发 NLLB-200 原生 ONNX 集成
```

---

## 参考资料

- [Helsinki-NLP OPUS-MT](https://huggingface.co/Helsinki-NLP)
- [Meta NLLB-200](https://huggingface.co/facebook/nllb-200-distilled-600M)
- [Optimum ONNX Export](https://huggingface.co/docs/optimum/exporters/onnx/overview)
- [tokenizers crate](https://docs.rs/tokenizers)
- [Ollama](https://ollama.com)

# Handy 系统配置要求（二开分发参考）

> 本文档基于 Handy 官方 README 的推荐配置，结合 Whisper.cpp / Silero VAD / Tauri 这些组件的实际运行特性，给出**经验性最低配置预估**，用于二开项目的硬件选型与用户告知。
>
> 官方原话："如果不满足推荐配置，性能会下降"，并未给出绝对硬性门槛。以下配置为工程经验推导，非官方承诺。

---

## 一、官方推荐配置（原文）

### 平台支持
- **macOS**（Intel 与 Apple Silicon）
- **x64 Windows**
- **x64 Linux**（Ubuntu 22.04 / 24.04）

### 按模型类型划分

**Whisper 模型**（Small / Medium / Turbo / Large）
- macOS：M 系列芯片 Mac（Metal 加速），或 Intel Mac
- Windows：CPU 推理（已移除 Vulkan 链接，兼容性优先；旧版 Win10 LTSC / 旧显卡驱动机器也能跑起来）
- Linux：Intel、AMD 或 NVIDIA GPU（OpenBLAS + Vulkan）
- ⚠️ 已知问题：Whisper 模型在某些 Linux 配置上会崩溃，与具体硬件配置相关

**Parakeet V3 模型**（面向配置较低的电脑）
- 纯 CPU 运行，硬件兼容性广
- 最低要求：Intel 第 6 代 Skylake 或同等级 AMD 处理器
- 性能：中端 i5 上可达约 5x 实时速度
- 自动语言识别，无需手动选择

### 模型体积参考

| 模型 | 大小 | 类型 |
|---|---|---|
| Whisper Small | 487 MB | GPU 加速 |
| Whisper Medium | 492 MB | GPU 加速 |
| Whisper Turbo | 1600 MB | GPU 加速 |
| Whisper Large | 1100 MB | GPU 加速 |
| Parakeet V3 | 478 MB | 纯 CPU |

---

## 二、绝对最低门槛（全平台通用）

| 项 | 要求 | 原因 |
|---|---|---|
| **CPU** | Intel Skylake（6 代）/ AMD Ryzen 1 代 及以上，≥ 4 核 | Parakeet 明确要求；也是 Vulkan 驱动的实际下限 |
| **内存** | 8 GB RAM | Tauri + WebView 约 500 MB，Rust 后端约 200 MB，其余留给模型 |
| **存储** | SSD，≥ 10 GB 可用空间 | HDD 加载 1.6 GB 模型会慢到不可用 |
| **操作系统** | Windows 10 / macOS 11 / Ubuntu 22.04 及以上 | Tauri 2.x 最低支持 |
| **麦克风** | 任何系统可识别的录音设备 | — |

---

## 三、按模型分档的最低配置预估

### 🟢 Parakeet V3（CPU，478 MB）— 最低门槛方案

| 项 | 配置 |
|---|---|
| CPU | i5-6300U / Ryzen 3 2200U 及以上 |
| 内存 | 8 GB（实占约 1.5–2 GB） |
| GPU | 不需要 |

**适用场景**：近十年内的办公本、无独立显卡的机器。
**实测性能**：中端 i5 可达 5x 实时速度，即说一句话等约 0.2 句话时间。

---

### 🟡 Whisper Small（487 MB）— 中等精度方案

| 项 | 配置 |
|---|---|
| CPU | i5-8 代 / Ryzen 3000 系列（Windows 走 CPU 推理） |
| 内存 | 8 GB |
| GPU | Windows 不依赖；Linux/macOS 可享受 GPU 加速 |
| VRAM | — |

---

### 🟠 Whisper Medium（492 MB）/ Turbo（1.6 GB）

| 项 | 配置 |
|---|---|
| CPU | i5-10 代 / Ryzen 5 5600 及以上 |
| 内存 | 16 GB（Turbo 加载 + KV cache 可吃 3–4 GB） |
| GPU | NVIDIA GTX 1650 / RTX 2060 及以上<br>或 AMD RX 5600 及以上<br>或 Apple M1 及以上 |
| VRAM | ≥ 4 GB |

---

### 🔴 Whisper Large（1.1 GB，q5_0 量化）

| 项 | 配置 |
|---|---|
| CPU | i7-10 代 / Ryzen 7 及以上 |
| 内存 | 16 GB（建议 32 GB） |
| GPU | NVIDIA RTX 3060 / RTX 4060 及以上<br>或 Apple M1 Pro / M2 及以上 |
| VRAM | ≥ 6 GB |

---

## 四、特别提醒（基于 README 已知问题）

1. **Windows 已移除 Vulkan 链接**：handy.exe 不再依赖 `vulkan-1.dll`，旧 Win10 LTSC / 旧显卡驱动机器不会再因为 Vulkan 1.0 loader 而启动失败（`vkGetPhysicalDeviceFeatures2` 入口缺失）。代价是 Windows 走 CPU 推理。
2. **Linux 显示服务器依赖**：
   - X11：需安装 `xdotool`
   - Wayland：需安装 `wtype` 或 `dotool`，否则文字粘贴会失败
3. **机械硬盘（HDD）不建议**：Turbo / Large 模型启动会非常慢。
4. **Linux 缺运行时库**：报 `libgtk-layer-shell.so.0` 错误时需按发行版安装 `libgtk-layer-shell0` 等包。

---

## 五、二开分发建议

### 推荐默认配置

面向企业客户或员工机器分发时，推荐的**最低配置门槛**：

> **Intel 第 6 代 i5 / 8 GB RAM / SSD / Windows 10 或更新版本**

这是能覆盖约 95% 办公机器的安全门槛。

### 模型策略

| 角色 | 默认模型 | 理由 |
|---|---|---|
| 普通办公机（无独显） | Parakeet V3 | 兼容性最好、崩溃风险最低 |
| 带独显的机器 | Whisper Turbo / Large | 识别精度更高 |
| 不确定配置的用户 | Parakeet V3 | 走稳妥路线 |

### 可选的增强方向

- **首次启动硬件检测**：自动识别 CPU 代次、GPU、内存，推荐合适模型。
- **模型按需下载**：首次选择模型时再下载，减少安装包体积。
- **降级策略**：Whisper 崩溃时自动回退到 Parakeet V3。

---

## 六、参考链接

- Handy 官方仓库：https://github.com/cjpais/Handy
- 官方 README 系统要求章节：README.md 第 210–226 行
- 模型下载 CDN：`https://blob.handy.computer/`

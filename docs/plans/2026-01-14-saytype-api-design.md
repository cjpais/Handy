# SayType HTTP API 設計文件

> 日期：2026-01-14
> 狀態：待實作

## 概述

讓 Handy 桌面應用程式對外提供語音轉文字 API，供手機端 App 透過區域網路呼叫。

## 範圍

包含：
- axum HTTP 伺服器（含 CORS 支援）
- `GET /api/status` - 伺服器狀態查詢
- `POST /api/transcribe` - 語音轉文字
- 音訊格式轉換（Base64 解碼、WAV/WebM → f32）
- Token 認證機制
- React 前端設定頁面（開關、埠號、Token 顯示）
- 首次使用引導流程

不包含：
- LLM 潤飾功能（保留介面，`polish` 參數接受但不處理）
- Android 鍵盤 App
- WebSocket 即時串流
- mDNS 服務發現

## 檔案結構

```
src-tauri/src/saytype/
├── mod.rs           # 模組入口
├── api_server.rs    # HTTP 伺服器啟動邏輯
├── handlers.rs      # API 請求處理器
├── types.rs         # 請求/回應類型（已完成）
├── audio_convert.rs # 音訊格式轉換
└── config.rs        # API 設定管理

src/components/settings/
└── SayTypeSettings.tsx  # 設定介面
```

---

## API 端點設計

### GET /api/status

查詢伺服器狀態，用於手機端確認連線。

```
Request:
  Headers:
    Authorization: Bearer <token>

Response 200:
{
  "status": "ready" | "loading" | "error",
  "model_loaded": true,
  "current_model": "whisper-small",
  "version": "0.6.11"
}

Response 401:
{
  "error": "Invalid token",
  "code": "UNAUTHORIZED"
}
```

### POST /api/transcribe

執行語音轉文字。

```
Request:
  Headers:
    Authorization: Bearer <token>
    Content-Type: application/json
  Body:
{
  "audio_base64": "UklGRi4AAABXQVZFZm10...",
  "format": "wav" | "webm",
  "sample_rate": 16000,        // optional, 預設 16000
  "polish": false              // 保留介面，目前不處理
}

Response 200:
{
  "success": true,
  "raw_text": "你好世界",
  "polished_text": "你好世界",  // 目前與 raw_text 相同
  "language": "zh",
  "processing_time_ms": 1234
}

Response 400:
{
  "error": "Invalid audio format",
  "code": "INVALID_FORMAT"
}
```

### 錯誤代碼

| Code | 說明 |
|------|------|
| UNAUTHORIZED | Token 無效或未提供 |
| INVALID_FORMAT | 音訊格式不支援 |
| DECODE_ERROR | Base64 解碼失敗 |
| MODEL_NOT_LOADED | 模型尚未載入 |
| TRANSCRIBE_ERROR | 轉錄過程發生錯誤 |

---

## 音訊處理流程

### 處理管線

```
手機音訊 (Base64)
    ↓
1. Base64 解碼 → Vec<u8>
    ↓
2. 格式判斷 (WAV / WebM)
    ↓
3. 解碼為 PCM samples
   - WAV: 直接讀取 PCM data
   - WebM/Opus: 使用 opus 解碼器
    ↓
4. 重採樣至 16kHz mono (若需要)
    ↓
5. 轉換為 Vec<f32> (-1.0 ~ 1.0)
    ↓
6. 傳入 TranscriptionManager::transcribe()
```

### audio_convert.rs 介面

```rust
pub enum AudioFormat {
    Wav,
    WebM,
}

pub struct AudioConvertResult {
    pub samples: Vec<f32>,      // 16kHz mono
    pub duration_ms: u64,
}

/// 從 Base64 字串轉換為可用於轉錄的 f32 samples
pub fn convert_from_base64(
    base64_data: &str,
    format: AudioFormat,
) -> Result<AudioConvertResult, AudioConvertError>;

/// 從原始 bytes 轉換
pub fn convert_from_bytes(
    bytes: &[u8],
    format: AudioFormat,
) -> Result<AudioConvertResult, AudioConvertError>;
```

### 依賴套件

```toml
# Cargo.toml 新增
base64 = "0.22"
ogg = "0.9"           # WebM/OGG 容器解析
opus = "0.3"          # Opus 音訊解碼
```

註：專案已有 `resampler.rs`，可直接複用現有的重採樣邏輯。

---

## 設定與啟動流程

### 設定項目 (config.rs)

```rust
#[derive(Serialize, Deserialize, Clone)]
pub struct SayTypeConfig {
    /// API 是否啟用
    pub enabled: bool,
    /// 監聽埠號 (預設 8765)
    pub port: u16,
    /// 認證 Token (首次啟用時自動產生)
    pub token: String,
    /// 是否已完成首次設定引導
    pub onboarded: bool,
}

impl Default for SayTypeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 8765,
            token: generate_random_token(),  // 32 字元隨機字串
            onboarded: false,
        }
    }
}
```

### 儲存位置

整合至現有的 `tauri-plugin-store`，存放於 settings store：

```json
{
  "saytype": {
    "enabled": true,
    "port": 8765,
    "token": "a1b2c3d4...",
    "onboarded": true
  }
}
```

### 啟動流程

```
應用程式啟動
    ↓
讀取 SayTypeConfig
    ↓
┌─ enabled == true ─────────────────┐
│       ↓                           │
│  啟動 API Server (背景 thread)     │
│       ↓                           │
│  log: "SayType API listening on   │
│        http://0.0.0.0:8765"       │
└───────────────────────────────────┘
    ↓
┌─ enabled == false ────────────────┐
│  不啟動，等待使用者在設定中開啟    │
└───────────────────────────────────┘
```

### Token 產生

```rust
fn generate_random_token() -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..32)
        .map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char)
        .collect()
}
```

---

## 前端設定介面

### 首次使用引導

當 `onboarded == false` 且使用者進入 SayType 設定頁面時，顯示引導對話框：

```
┌─────────────────────────────────────────┐
│  啟用 SayType 遠端輸入？                 │
├─────────────────────────────────────────┤
│  SayType 可讓你的手機透過區域網路        │
│  使用這台電腦的語音轉文字功能。          │
│                                         │
│  啟用後，同一網路內的裝置可透過          │
│  API 連線（需要認證 Token）。            │
│                                         │
│  [暫時不要]              [啟用 SayType]  │
└─────────────────────────────────────────┘
```

### SayTypeSettings.tsx 介面

```
┌─ SayType 遠端輸入 ──────────────────────┐
│                                         │
│  啟用 API 伺服器          [開關 Toggle] │
│                                         │
│  ─────────────────────────────────────  │
│                                         │
│  連線資訊（啟用時顯示）                  │
│                                         │
│  伺服器位址                              │
│  ┌─────────────────────────────────┐    │
│  │ http://192.168.1.100:8765      │ 📋 │
│  └─────────────────────────────────┘    │
│                                         │
│  認證 Token                             │
│  ┌─────────────────────────────────┐    │
│  │ a1b2c3d4e5f6g7h8...            │ 👁📋│
│  └─────────────────────────────────┘    │
│  [重新產生 Token]                        │
│                                         │
│  埠號                                    │
│  ┌──────┐                               │
│  │ 8765 │  (修改後需重啟 API)           │
│  └──────┘                               │
│                                         │
└─────────────────────────────────────────┘
```

### 功能說明

| 元件 | 行為 |
|------|------|
| 開關 Toggle | 即時啟動/停止 API Server |
| 📋 複製按鈕 | 複製位址/Token 到剪貼簿 |
| 👁 顯示按鈕 | 切換 Token 明碼/遮罩顯示 |
| 重新產生 Token | 產生新 Token，舊的立即失效 |
| 埠號輸入 | 數字輸入，範圍 1024-65535 |

---

## 實作清單

### 後端 (Rust)

| 檔案 | 任務 | 依賴 |
|------|------|------|
| `config.rs` | SayTypeConfig 結構與讀寫 | - |
| `audio_convert.rs` | Base64 解碼、WAV/WebM 轉 f32 | config |
| `handlers.rs` | status、transcribe handler 實作 | audio_convert |
| `api_server.rs` | axum Router、CORS、Token middleware | handlers |
| `mod.rs` | 整合啟動邏輯 | api_server |
| `lib.rs` | 應用啟動時呼叫 saytype 初始化 | mod |
| `commands/saytype.rs` | 前端 Tauri commands | config |

### 前端 (React/TypeScript)

| 檔案 | 任務 | 依賴 |
|------|------|------|
| `SayTypeSettings.tsx` | 設定頁面 UI | - |
| `SayTypeOnboarding.tsx` | 首次使用引導 Dialog | - |
| `useSayType.ts` | Hook：讀寫設定、控制伺服器 | Tauri commands |
| `i18n/locales/*/translation.json` | 翻譯字串 | - |

### Tauri Commands

```typescript
// bindings.ts 預期新增
invoke('saytype_get_config') → SayTypeConfig
invoke('saytype_set_config', { config }) → void
invoke('saytype_start_server') → void
invoke('saytype_stop_server') → void
invoke('saytype_regenerate_token') → string
invoke('saytype_get_local_ip') → string
```

### 新增依賴

```toml
# src-tauri/Cargo.toml
axum = "0.7"
tower-http = { version = "0.5", features = ["cors"] }
base64 = "0.22"
ogg = "0.9"
opus = "0.3"
local-ip-address = "0.6"
```

---

## 建議實作順序

1. `config.rs` - 設定結構
2. `audio_convert.rs` - 音訊轉換
3. `handlers.rs` - API handlers
4. `api_server.rs` - 伺服器啟動
5. `commands/saytype.rs` - Tauri commands
6. `SayTypeSettings.tsx` - 前端設定頁
7. `SayTypeOnboarding.tsx` - 首次引導
8. 整合測試

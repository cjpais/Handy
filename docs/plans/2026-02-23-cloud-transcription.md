# Cloud Transcription (OpenAI-compatible) Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Добавить поддержку облачной транскрипции через OpenAI-совместимые Whisper API (Groq, OpenAI, Custom) как альтернативу локальным моделям.

**Architecture:** Добавляем новый вариант `CloudApi(CloudApiClient)` в существующий `LoadedEngine` enum в `TranscriptionManager`. При выборе облачного провайдера "загрузка" модели мгновенна (просто инициализация HTTP-клиента), после чего `transcribe()` шлёт WAV через `multipart/form-data` на `/v1/audio/transcriptions`. Настройки провайдеров хранятся по тому же паттерну, что и `post_process_*` поля в `AppSettings`.

**Tech Stack:** Rust (`reqwest` multipart, `hound` WAV — оба уже в `Cargo.toml`), React/TypeScript (Zustand + Tauri commands), `tauri-specta` (автогенерация `bindings.ts`).

---

## Обзор изменений по файлам

| Файл                                                     | Тип         | Что меняем                     |
| -------------------------------------------------------- | ----------- | ------------------------------ |
| `src-tauri/src/cloud_transcription.rs`                   | **Создать** | Типы провайдеров + HTTP клиент |
| `src-tauri/src/settings.rs`                              | Изменить    | 4 новых поля + миграция        |
| `src-tauri/src/managers/transcription.rs`                | Изменить    | `CloudApi` вариант + match arm |
| `src-tauri/src/commands/cloud_transcription.rs`          | **Создать** | 8 Tauri команд                 |
| `src-tauri/src/commands/mod.rs`                          | Изменить    | `pub mod cloud_transcription;` |
| `src-tauri/src/lib.rs`                                   | Изменить    | Зарегистрировать команды       |
| `src/components/settings/CloudTranscriptionSettings.tsx` | **Создать** | UI компонент                   |
| `src/components/settings/useCloudTranscriptionState.ts`  | **Создать** | Хук для состояния              |
| `src/components/settings/general/`                       | Изменить    | Встроить в settings layout     |
| `src/stores/settingsStore.ts`                            | Изменить    | Новые updater функции          |
| `src/i18n/locales/en/translation.json`                   | Изменить    | Новые ключи                    |

---

## Задача 1: Типы и HTTP клиент — `cloud_transcription.rs`

**Файлы:**

- Создать: `src-tauri/src/cloud_transcription.rs`

Этот файл содержит всю логику облачной транскрипции: определения типов провайдеров, список встроенных провайдеров, конвертацию аудио в WAV и HTTP клиент.

### Шаг 1.1: Создать файл с типами провайдеров

```rust
// src-tauri/src/cloud_transcription.rs

use hound::{SampleFormat, WavSpec, WavWriter};
use log::{debug, error};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;
use std::io::Cursor;

/// Описание одной модели облачного провайдера.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CloudTranscriptionModel {
    /// Идентификатор модели, передаётся в поле `model` запроса
    pub id: String,
    /// Отображаемое название
    pub label: String,
    /// Поддерживаемые языки (пустой вектор = все языки)
    pub languages: Vec<String>,
    /// Поддерживает ли модель перевод на английский (task=translate)
    pub supports_translation: bool,
}

/// Метаданные облачного провайдера транскрипции.
/// Структура намеренно параллельна `PostProcessProvider` из settings.rs.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CloudTranscriptionProvider {
    pub id: String,
    pub label: String,
    pub base_url: String,
    /// Разрешено ли пользователю редактировать base_url (только для "custom")
    pub allow_base_url_edit: bool,
    /// Предустановленные модели. Пустой вектор = список фетчится через /models
    pub static_models: Vec<CloudTranscriptionModel>,
    /// Нужен ли API ключ
    pub requires_api_key: bool,
}
```

### Шаг 1.2: Добавить список встроенных провайдеров

```rust
/// Возвращает список встроенных провайдеров облачной транскрипции.
/// Вызывается как из команды get_cloud_transcription_providers,
/// так и из ensure_cloud_transcription_defaults при миграции настроек.
pub fn builtin_providers() -> Vec<CloudTranscriptionProvider> {
    vec![
        CloudTranscriptionProvider {
            id: "groq".to_string(),
            label: "Groq".to_string(),
            base_url: "https://api.groq.com/openai/v1".to_string(),
            allow_base_url_edit: false,
            static_models: vec![
                CloudTranscriptionModel {
                    id: "whisper-large-v3-turbo".to_string(),
                    label: "Whisper Large v3 Turbo".to_string(),
                    languages: vec![],
                    supports_translation: false,
                },
                CloudTranscriptionModel {
                    id: "whisper-large-v3".to_string(),
                    label: "Whisper Large v3".to_string(),
                    languages: vec![],
                    supports_translation: false,
                },
                CloudTranscriptionModel {
                    id: "distil-whisper-large-v3-en".to_string(),
                    label: "Distil Whisper (English only)".to_string(),
                    languages: vec!["en".to_string()],
                    supports_translation: false,
                },
            ],
            requires_api_key: true,
        },
        CloudTranscriptionProvider {
            id: "openai".to_string(),
            label: "OpenAI".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            allow_base_url_edit: false,
            static_models: vec![
                CloudTranscriptionModel {
                    id: "whisper-1".to_string(),
                    label: "Whisper-1".to_string(),
                    languages: vec![],
                    supports_translation: true,
                },
                CloudTranscriptionModel {
                    id: "gpt-4o-mini-transcribe".to_string(),
                    label: "GPT-4o Mini Transcribe".to_string(),
                    languages: vec![],
                    supports_translation: false,
                },
                CloudTranscriptionModel {
                    id: "gpt-4o-transcribe".to_string(),
                    label: "GPT-4o Transcribe".to_string(),
                    languages: vec![],
                    supports_translation: false,
                },
            ],
            requires_api_key: true,
        },
        // Custom — пользователь задаёт base_url сам
        CloudTranscriptionProvider {
            id: "custom".to_string(),
            label: "Custom (OpenAI-compatible)".to_string(),
            base_url: "http://localhost:8000/v1".to_string(),
            allow_base_url_edit: true,
            static_models: vec![],  // пользователь вводит модель вручную
            requires_api_key: false,
        },
    ]
}
```

### Шаг 1.3: Добавить конвертацию f32 → WAV

```rust
/// Конвертирует f32 семплы (16kHz mono) в WAV байты для отправки в API.
/// Handy уже ресемплирует аудио до 16kHz mono перед транскрипцией,
/// поэтому просто пакуем готовые данные.
pub fn samples_to_wav(samples: &[f32], sample_rate: u32) -> Result<Vec<u8>, String> {
    let mut buf = Vec::with_capacity(samples.len() * 2 + 44);
    let cursor = Cursor::new(&mut buf);

    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };

    let mut writer = WavWriter::new(cursor, spec)
        .map_err(|e| format!("Не удалось создать WAV writer: {}", e))?;

    for &sample in samples {
        // Клипируем и конвертируем f32 [-1.0, 1.0] → i16
        let pcm = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        writer
            .write_sample(pcm)
            .map_err(|e| format!("Ошибка записи сэмпла: {}", e))?;
    }

    writer
        .finalize()
        .map_err(|e| format!("Ошибка финализации WAV: {}", e))?;

    Ok(buf)
}
```

### Шаг 1.4: Добавить HTTP клиент

```rust
/// HTTP клиент для одного облачного провайдера.
/// Создаётся в TranscriptionManager::load_model при выборе провайдера.
pub struct CloudApiClient {
    pub provider_id: String,
    pub provider_label: String,
    base_url: String,
    model_id: String,
    api_key: String,
    client: reqwest::Client,
}

impl CloudApiClient {
    pub fn new(
        provider: &CloudTranscriptionProvider,
        model_id: String,
        api_key: String,
        // Переопределение base_url (для custom провайдера из настроек)
        base_url_override: Option<String>,
    ) -> Result<Self, String> {
        let mut headers = HeaderMap::new();

        if !api_key.is_empty() {
            let bearer = format!("Bearer {}", api_key);
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&bearer)
                    .map_err(|e| format!("Некорректный API ключ: {}", e))?,
            );
        }

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| format!("Ошибка создания HTTP клиента: {}", e))?;

        let base_url = base_url_override
            .unwrap_or_else(|| provider.base_url.clone());

        Ok(Self {
            provider_id: provider.id.clone(),
            provider_label: provider.label.clone(),
            base_url,
            model_id,
            api_key,
            client,
        })
    }

    /// Отправляет аудио на /v1/audio/transcriptions и возвращает текст.
    pub async fn transcribe(
        &self,
        samples: &[f32],
        sample_rate: u32,
        language: &str,
        translate: bool,
    ) -> Result<String, String> {
        // 1. Кодируем аудио в WAV
        let wav_bytes = samples_to_wav(samples, sample_rate)?;
        debug!(
            "Отправка WAV {} байт на {} ({})",
            wav_bytes.len(),
            self.provider_label,
            self.model_id
        );

        // 2. Собираем multipart форму согласно OpenAI API spec
        let file_part = reqwest::multipart::Part::bytes(wav_bytes)
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .map_err(|e| format!("Ошибка MIME типа: {}", e))?;

        let mut form = reqwest::multipart::Form::new()
            .part("file", file_part)
            .text("model", self.model_id.clone())
            .text("response_format", "json");

        // language: если "auto" — поле не передаём (провайдер сам определит)
        if language != "auto" && !language.is_empty() {
            form = form.text("language", language.to_string());
        }

        // translate поддерживается только некоторыми провайдерами (openai whisper-1)
        if translate {
            form = form.text("task", "translate");
        }

        // 3. Отправляем запрос
        let url = format!(
            "{}/audio/transcriptions",
            self.base_url.trim_end_matches('/')
        );

        let response = self
            .client
            .post(&url)
            .multipart(form)
            .send()
            .await
            .map_err(|e| format!("HTTP запрос к {} провалился: {}", self.provider_label, e))?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "не удалось прочитать ответ".to_string());
            return Err(format!(
                "API {} вернул статус {}: {}",
                self.provider_label, status, body
            ));
        }

        // 4. Парсим ответ {"text": "..."}
        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Ошибка парсинга ответа от {}: {}", self.provider_label, e))?;

        let text = json["text"]
            .as_str()
            .ok_or_else(|| {
                format!(
                    "Ответ от {} не содержит поля 'text': {:?}",
                    self.provider_label, json
                )
            })?
            .trim()
            .to_string();

        debug!(
            "Облачная транскрипция ({}): получено {} символов",
            self.provider_label,
            text.len()
        );

        Ok(text)
    }
}
```

### Шаг 1.5: Зарегистрировать модуль в `lib.rs`

В `src-tauri/src/lib.rs` добавить в список модулей:

```rust
mod cloud_transcription;
mod commands;
// ... остальные модули
```

### Шаг 1.6: Коммит

```bash
git add src-tauri/src/cloud_transcription.rs src-tauri/src/lib.rs
git commit -m "feat: добавить CloudApiClient и типы провайдеров облачной транскрипции"
```

---

## Задача 2: Новые поля в `AppSettings` и миграция

**Файлы:**

- Изменить: `src-tauri/src/settings.rs`

Добавляем четыре поля в `AppSettings` по тому же паттерну, что и `post_process_*` поля. Пишем функцию `ensure_cloud_transcription_defaults()` для миграции существующих пользователей.

### Шаг 2.1: Добавить поля в `AppSettings`

Найти конец структуры `AppSettings` (перед `pub external_script_path`) и вставить:

```rust
// --- Облачная транскрипция ---
/// Использовать облачный провайдер вместо локальной модели
#[serde(default = "default_use_cloud_transcription")]
pub use_cloud_transcription: bool,

/// ID выбранного облачного провайдера ("groq", "openai", "custom")
#[serde(default = "default_cloud_transcription_provider_id")]
pub cloud_transcription_provider_id: String,

/// provider_id → API ключ
#[serde(default = "default_cloud_transcription_api_keys")]
pub cloud_transcription_api_keys: HashMap<String, String>,

/// provider_id → выбранная модель
#[serde(default = "default_cloud_transcription_models")]
pub cloud_transcription_models: HashMap<String, String>,

/// provider_id → кастомный base_url (только для "custom" провайдера)
#[serde(default)]
pub cloud_transcription_base_urls: HashMap<String, String>,
```

### Шаг 2.2: Добавить default функции

```rust
fn default_use_cloud_transcription() -> bool {
    false
}

fn default_cloud_transcription_provider_id() -> String {
    "groq".to_string()
}

fn default_cloud_transcription_api_keys() -> HashMap<String, String> {
    // Пустой ключ для каждого встроенного провайдера
    crate::cloud_transcription::builtin_providers()
        .into_iter()
        .map(|p| (p.id, String::new()))
        .collect()
}

fn default_cloud_transcription_models() -> HashMap<String, String> {
    // Дефолтная модель — первая в списке static_models провайдера
    crate::cloud_transcription::builtin_providers()
        .into_iter()
        .map(|p| {
            let default_model = p
                .static_models
                .first()
                .map(|m| m.id.clone())
                .unwrap_or_default();
            (p.id, default_model)
        })
        .collect()
}
```

### Шаг 2.3: Добавить функцию миграции

После `ensure_post_process_defaults()` добавить:

```rust
/// Добавляет отсутствующие ключи для провайдеров облачной транскрипции.
/// Вызывается при каждом get_settings() — безопасно для идемпотентности.
fn ensure_cloud_transcription_defaults(settings: &mut AppSettings) -> bool {
    let mut changed = false;

    for provider in crate::cloud_transcription::builtin_providers() {
        // Убедиться, что есть запись для API ключа
        if !settings.cloud_transcription_api_keys.contains_key(&provider.id) {
            settings
                .cloud_transcription_api_keys
                .insert(provider.id.clone(), String::new());
            changed = true;
        }

        // Убедиться, что есть запись для модели
        if !settings.cloud_transcription_models.contains_key(&provider.id) {
            let default_model = provider
                .static_models
                .first()
                .map(|m| m.id.clone())
                .unwrap_or_default();
            settings
                .cloud_transcription_models
                .insert(provider.id.clone(), default_model);
            changed = true;
        }
    }

    changed
}
```

### Шаг 2.4: Вызвать миграцию в `get_settings()`

Найти функцию `get_settings()` и добавить вызов после `ensure_post_process_defaults()`:

```rust
pub fn get_settings(app: &AppHandle) -> AppSettings {
    // ... существующий код загрузки ...

    ensure_post_process_defaults(&mut settings);
    // ДОБАВИТЬ:
    ensure_cloud_transcription_defaults(&mut settings);

    settings
}
```

То же самое в `load_or_create_app_settings()`.

### Шаг 2.5: Коммит

```bash
git add src-tauri/src/settings.rs
git commit -m "feat: добавить поля cloud_transcription в AppSettings с миграцией"
```

---

## Задача 3: `CloudApi` вариант в `TranscriptionManager`

**Файлы:**

- Изменить: `src-tauri/src/managers/transcription.rs`

Добавляем `CloudApi` как 6-й вариант `LoadedEngine`. Для него `load_model()` создаёт HTTP клиент мгновенно (не нужна тяжёлая загрузка GGML модели). `transcribe()` получает новый match arm.

### Шаг 3.1: Импорт и новый вариант

В начало `transcription.rs` добавить импорт:

```rust
use crate::cloud_transcription::CloudApiClient;
```

В `enum LoadedEngine` добавить вариант:

```rust
enum LoadedEngine {
    Whisper(WhisperEngine),
    Parakeet(ParakeetEngine),
    Moonshine(MoonshineEngine),
    MoonshineStreaming(MoonshineStreamingEngine),
    SenseVoice(SenseVoiceEngine),
    // НОВЫЙ: HTTP клиент для облачной транскрипции
    CloudApi(CloudApiClient),
}
```

### Шаг 3.2: Ветка CloudApi в `load_model()`

В методе `load_model()` найти блок `match model_info.engine_type` и добавить ветку ДО закрывающей скобки. Но сначала надо обработать особый случай: при `use_cloud_transcription = true` мы вообще не смотрим на `engine_type` из `ModelManager`.

Добавить в начало `load_model()` (перед вызовом `model_manager.get_model_info()`):

```rust
pub fn load_model(&self, model_id: &str) -> Result<()> {
    let load_start = std::time::Instant::now();
    debug!("Начало загрузки модели: {}", model_id);

    // Эмитим событие о начале загрузки
    let _ = self.app_handle.emit("model-state-changed", ModelStateEvent {
        event_type: "loading_started".to_string(),
        model_id: Some(model_id.to_string()),
        model_name: None,
        error: None,
    });

    let settings = get_settings(&self.app_handle);

    // --- НОВЫЙ БЛОК: Облачная транскрипция ---
    if settings.use_cloud_transcription {
        return self.load_cloud_provider(&settings);
    }
    // --- конец нового блока ---

    // Остальной существующий код load_model() без изменений...
```

Добавить приватный метод `load_cloud_provider()`:

```rust
/// Инициализирует HTTP клиент для выбранного облачного провайдера.
/// "Загрузка" мгновенна — просто создаём reqwest::Client.
fn load_cloud_provider(&self, settings: &crate::settings::AppSettings) -> Result<()> {
    use crate::cloud_transcription::{builtin_providers, CloudApiClient};

    let provider_id = &settings.cloud_transcription_provider_id;

    // Найти провайдера в списке встроенных
    let provider = builtin_providers()
        .into_iter()
        .find(|p| &p.id == provider_id)
        .ok_or_else(|| anyhow::anyhow!("Провайдер не найден: {}", provider_id))?;

    let api_key = settings
        .cloud_transcription_api_keys
        .get(provider_id)
        .cloned()
        .unwrap_or_default();

    let model_id = settings
        .cloud_transcription_models
        .get(provider_id)
        .cloned()
        .unwrap_or_else(|| {
            provider
                .static_models
                .first()
                .map(|m| m.id.clone())
                .unwrap_or_else(|| "whisper-1".to_string())
        });

    // Переопределение base_url для кастомного провайдера
    let base_url_override = if provider.allow_base_url_edit {
        settings
            .cloud_transcription_base_urls
            .get(provider_id)
            .filter(|u| !u.is_empty())
            .cloned()
    } else {
        None
    };

    let client = CloudApiClient::new(&provider, model_id.clone(), api_key, base_url_override)
        .map_err(|e| anyhow::anyhow!("Не удалось создать клиент провайдера {}: {}", provider_id, e))?;

    // Сохраняем "клиент" как загруженный движок
    {
        let mut engine = self.lock_engine();
        *engine = Some(LoadedEngine::CloudApi(client));
    }
    {
        let mut current_model = self.current_model_id.lock().unwrap();
        // ID в формате "cloud:{provider_id}:{model_id}" для консистентности
        *current_model = Some(format!("cloud:{}:{}", provider_id, model_id));
    }

    // Эмитим успешную загрузку — UI обновится
    let _ = self.app_handle.emit("model-state-changed", ModelStateEvent {
        event_type: "loading_completed".to_string(),
        model_id: Some(format!("cloud:{}:{}", provider_id, model_id)),
        model_name: Some(format!("{} — {}", provider.label, model_id)),
        error: None,
    });

    debug!(
        "Облачный провайдер '{}' готов ({}ms)",
        provider_id,
        std::time::Instant::now().elapsed().as_millis()
    );

    Ok(())
}
```

### Шаг 3.3: Добавить match arm в `transcribe()`

Найти в `transcribe()` `catch_unwind` блок с матчем по `LoadedEngine`. После последнего существующего варианта (`SenseVoice`) добавить:

```rust
LoadedEngine::CloudApi(cloud_client) => {
    let language = settings.selected_language.as_str();
    let translate = settings.translate_to_english;
    let audio_clone = audio.clone();

    // cloud_client.transcribe() — async, поэтому используем block_in_place
    // чтобы не требовать #[tokio::main] у всего TranscriptionManager
    tokio::task::block_in_place(|| {
        tauri::async_runtime::block_on(async {
            cloud_client
                .transcribe(&audio_clone, 16000, language, translate)
                .await
                .map(|text| transcribe_rs::TranscriptionResult {
                    text,
                    segments: vec![],
                })
                .map_err(|e| anyhow::anyhow!("Облачная транскрипция: {}", e))
        })
    })
}
```

### Шаг 3.4: Добавить ветку в `unload_model()`

Найти `match loaded_engine` в `unload_model()` и добавить:

```rust
LoadedEngine::CloudApi(_) => {
    // HTTP клиент не требует явной выгрузки — просто дропаем
    debug!("Облачный HTTP клиент освобождён");
}
```

### Шаг 3.5: Обновить `initiate_model_load()`

`initiate_model_load()` сейчас читает `settings.selected_model`. При облачном режиме передаём специальный ID:

```rust
pub fn initiate_model_load(&self) {
    let mut is_loading = self.is_loading.lock().unwrap();
    if *is_loading || self.is_model_loaded() {
        return;
    }

    *is_loading = true;
    let self_clone = self.clone();
    thread::spawn(move || {
        let settings = get_settings(&self_clone.app_handle);

        // НОВОЕ: при облачном режиме передаём специальный ID
        let model_id = if settings.use_cloud_transcription {
            format!("cloud:{}", settings.cloud_transcription_provider_id)
        } else {
            settings.selected_model.clone()
        };

        if let Err(e) = self_clone.load_model(&model_id) {
            error!("Ошибка загрузки модели: {}", e);
        }
        let mut is_loading = self_clone.is_loading.lock().unwrap();
        *is_loading = false;
        self_clone.loading_condvar.notify_all();
    });
}
```

### Шаг 3.6: Коммит

```bash
git add src-tauri/src/managers/transcription.rs
git commit -m "feat: добавить CloudApi вариант в LoadedEngine и load_cloud_provider()"
```

---

## Задача 4: Tauri команды

**Файлы:**

- Создать: `src-tauri/src/commands/cloud_transcription.rs`
- Изменить: `src-tauri/src/commands/mod.rs`
- Изменить: `src-tauri/src/lib.rs`

### Шаг 4.1: Создать файл команд

```rust
// src-tauri/src/commands/cloud_transcription.rs

use crate::cloud_transcription::{builtin_providers, CloudApiClient, CloudTranscriptionProvider};
use crate::managers::transcription::TranscriptionManager;
use crate::settings::{get_settings, write_settings};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

/// Возвращает список встроенных облачных провайдеров с актуальными base_url
/// (для кастомного провайдера подставляем значение из настроек).
#[tauri::command]
#[specta::specta]
pub fn get_cloud_transcription_providers(
    app: AppHandle,
) -> Result<Vec<CloudTranscriptionProvider>, String> {
    let settings = get_settings(&app);
    let mut providers = builtin_providers();

    // Для кастомного провайдера подставляем сохранённый base_url
    for provider in &mut providers {
        if provider.allow_base_url_edit {
            if let Some(url) = settings.cloud_transcription_base_urls.get(&provider.id) {
                if !url.is_empty() {
                    provider.base_url = url.clone();
                }
            }
        }
    }

    Ok(providers)
}

/// Включает или выключает облачный режим транскрипции.
/// При включении выгружает текущую локальную модель.
#[tauri::command]
#[specta::specta]
pub fn set_use_cloud_transcription(
    app: AppHandle,
    enabled: bool,
) -> Result<(), String> {
    // Выгрузить текущий движок при переключении режима
    if let Some(tm) = app.try_state::<Arc<TranscriptionManager>>() {
        if let Err(e) = tm.unload_model() {
            log::warn!("Ошибка выгрузки модели при смене режима: {}", e);
        }
    }

    let mut settings = get_settings(&app);
    settings.use_cloud_transcription = enabled;
    write_settings(&app, &settings).map_err(|e| e.to_string())
}

/// Устанавливает активный провайдер и сбрасывает загруженный движок.
#[tauri::command]
#[specta::specta]
pub fn set_cloud_transcription_provider(
    app: AppHandle,
    provider_id: String,
) -> Result<(), String> {
    // Проверяем, что провайдер существует
    let exists = builtin_providers().iter().any(|p| p.id == provider_id);
    if !exists {
        return Err(format!("Неизвестный провайдер: {}", provider_id));
    }

    // Выгружаем текущий движок — он будет перезагружен с новым провайдером
    if let Some(tm) = app.try_state::<Arc<TranscriptionManager>>() {
        let _ = tm.unload_model();
    }

    let mut settings = get_settings(&app);
    settings.cloud_transcription_provider_id = provider_id;
    write_settings(&app, &settings).map_err(|e| e.to_string())
}

/// Устанавливает выбранную модель для провайдера.
#[tauri::command]
#[specta::specta]
pub fn set_cloud_transcription_model(
    app: AppHandle,
    provider_id: String,
    model_id: String,
) -> Result<(), String> {
    // Выгружаем — модель изменилась, надо переинициализировать клиент
    if let Some(tm) = app.try_state::<Arc<TranscriptionManager>>() {
        let _ = tm.unload_model();
    }

    let mut settings = get_settings(&app);
    settings
        .cloud_transcription_models
        .insert(provider_id, model_id);
    write_settings(&app, &settings).map_err(|e| e.to_string())
}

/// Сохраняет API ключ для провайдера.
#[tauri::command]
#[specta::specta]
pub fn set_cloud_transcription_api_key(
    app: AppHandle,
    provider_id: String,
    api_key: String,
) -> Result<(), String> {
    // Выгружаем — ключ мог измениться, клиент надо пересоздать
    if let Some(tm) = app.try_state::<Arc<TranscriptionManager>>() {
        let _ = tm.unload_model();
    }

    let mut settings = get_settings(&app);
    settings
        .cloud_transcription_api_keys
        .insert(provider_id, api_key);
    write_settings(&app, &settings).map_err(|e| e.to_string())
}

/// Устанавливает кастомный base_url (только для провайдеров с allow_base_url_edit=true).
#[tauri::command]
#[specta::specta]
pub fn set_cloud_transcription_base_url(
    app: AppHandle,
    provider_id: String,
    base_url: String,
) -> Result<(), String> {
    // Проверяем, что провайдер поддерживает кастомный URL
    let provider = builtin_providers()
        .into_iter()
        .find(|p| p.id == provider_id)
        .ok_or_else(|| format!("Провайдер не найден: {}", provider_id))?;

    if !provider.allow_base_url_edit {
        return Err(format!(
            "Провайдер '{}' не поддерживает изменение base_url",
            provider_id
        ));
    }

    if let Some(tm) = app.try_state::<Arc<TranscriptionManager>>() {
        let _ = tm.unload_model();
    }

    let mut settings = get_settings(&app);
    settings
        .cloud_transcription_base_urls
        .insert(provider_id, base_url);
    write_settings(&app, &settings).map_err(|e| e.to_string())
}

/// Тестирует подключение, отправляя 1 секунду тишины.
/// Возвращает Ok("ok") если API отвечает корректно, Err с описанием ошибки.
#[tauri::command]
#[specta::specta]
pub async fn test_cloud_transcription_provider(
    provider_id: String,
    api_key: String,
    model_id: String,
    base_url_override: Option<String>,
) -> Result<String, String> {
    use crate::cloud_transcription::builtin_providers;

    let provider = builtin_providers()
        .into_iter()
        .find(|p| p.id == provider_id)
        .ok_or_else(|| format!("Провайдер не найден: {}", provider_id))?;

    let client = CloudApiClient::new(&provider, model_id, api_key, base_url_override)?;

    // 1 секунда тишины @16kHz — минимально валидный аудио запрос
    let silence: Vec<f32> = vec![0.001_f32; 16_000];

    // Отправляем с language="en" чтобы не тратить время на автодетект
    client.transcribe(&silence, 16_000, "en", false).await?;

    Ok("ok".to_string())
}

/// Фетчит список доступных моделей через /models эндпоинт провайдера.
/// Нужно только для кастомных провайдеров (у встроенных модели захардкожены).
#[tauri::command]
#[specta::specta]
pub async fn fetch_cloud_transcription_models(
    provider_id: String,
    api_key: String,
    base_url_override: Option<String>,
) -> Result<Vec<String>, String> {
    use crate::cloud_transcription::builtin_providers;
    use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};

    let provider = builtin_providers()
        .into_iter()
        .find(|p| p.id == provider_id)
        .ok_or_else(|| format!("Провайдер не найден: {}", provider_id))?;

    let base_url = base_url_override
        .unwrap_or_else(|| provider.base_url.clone());

    let url = format!("{}/models", base_url.trim_end_matches('/'));

    let mut headers = HeaderMap::new();
    if !api_key.is_empty() {
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", api_key))
                .map_err(|e| format!("Некорректный API ключ: {}", e))?,
        );
    }

    let client = reqwest::Client::builder()
        .default_headers(headers)
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Ошибка клиента: {}", e))?;

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Запрос к /models провалился: {}", e))?;

    if !response.status().is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Ошибка получения моделей: {}", body));
    }

    let parsed: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Ошибка парсинга: {}", e))?;

    let mut models = Vec::new();

    // OpenAI формат: { data: [{ id: "..." }] }
    if let Some(data) = parsed.get("data").and_then(|d| d.as_array()) {
        for entry in data {
            if let Some(id) = entry.get("id").and_then(|i| i.as_str()) {
                models.push(id.to_string());
            }
        }
    }
    // Массив строк: ["model1", "model2"]
    else if let Some(array) = parsed.as_array() {
        for entry in array {
            if let Some(s) = entry.as_str() {
                models.push(s.to_string());
            }
        }
    }

    Ok(models)
}
```

### Шаг 4.2: Добавить `pub mod` в `commands/mod.rs`

```rust
pub mod audio;
pub mod cloud_transcription;  // НОВОЕ
pub mod history;
pub mod models;
pub mod transcription;
```

### Шаг 4.3: Зарегистрировать команды в `lib.rs`

В `collect_commands!([...])` добавить перед `commands::cancel_operation`:

```rust
commands::cloud_transcription::get_cloud_transcription_providers,
commands::cloud_transcription::set_use_cloud_transcription,
commands::cloud_transcription::set_cloud_transcription_provider,
commands::cloud_transcription::set_cloud_transcription_model,
commands::cloud_transcription::set_cloud_transcription_api_key,
commands::cloud_transcription::set_cloud_transcription_base_url,
commands::cloud_transcription::test_cloud_transcription_provider,
commands::cloud_transcription::fetch_cloud_transcription_models,
```

### Шаг 4.4: Проверить компиляцию

```bash
cd /Users/ilyanovik/Documents/Projects/oss/Handy/src-tauri
cargo check 2>&1
```

Ожидаем: 0 ошибок.

### Шаг 4.5: Коммит

```bash
git add src-tauri/src/commands/cloud_transcription.rs \
        src-tauri/src/commands/mod.rs \
        src-tauri/src/lib.rs
git commit -m "feat: добавить Tauri команды для облачной транскрипции"
```

---

## Задача 5: Регенерация `bindings.ts`

**Файлы:**

- Изменить: `src/bindings.ts` (авто)

После компиляции в dev-режиме `tauri-specta` автоматически регенерирует `src/bindings.ts`. Новые типы `CloudTranscriptionProvider`, `CloudTranscriptionModel` и все 8 команд появятся в TypeScript.

### Шаг 5.1: Запустить dev build (только для генерации bindings)

```bash
cd /Users/ilyanovik/Documents/Projects/oss/Handy
# Запустить один раз — после генерации bindings можно прервать
bun run tauri dev
# Ctrl+C через 10-15 секунд после старта
```

### Шаг 5.2: Проверить регенерацию

```bash
grep "CloudTranscriptionProvider\|getCloudTranscriptionProviders\|setUseCloudTranscription" src/bindings.ts
```

Ожидаем: все новые типы и команды присутствуют.

### Шаг 5.3: Коммит

```bash
git add src/bindings.ts
git commit -m "chore: регенерировать bindings.ts с командами облачной транскрипции"
```

---

## Задача 6: Хук состояния `useCloudTranscriptionState.ts`

**Файлы:**

- Создать: `src/components/settings/useCloudTranscriptionState.ts`

По образцу `usePostProcessProviderState.ts`.

### Шаг 6.1: Создать хук

```typescript
// src/components/settings/useCloudTranscriptionState.ts

import { useCallback, useMemo, useState } from "react";
import { commands, type CloudTranscriptionProvider } from "@/bindings";
import { useSettingsStore } from "../../stores/settingsStore";

type CloudTranscriptionState = {
  // Данные
  providers: CloudTranscriptionProvider[];
  selectedProviderId: string;
  selectedProvider: CloudTranscriptionProvider | undefined;
  selectedModel: string;
  apiKey: string;
  baseUrl: string;
  useCloud: boolean;
  isCustomProvider: boolean;
  // Статус подключения
  testStatus: "idle" | "testing" | "ok" | "error";
  testError: string;
  // Хендлеры
  handleToggleCloud: (enabled: boolean) => Promise<void>;
  handleProviderSelect: (id: string) => Promise<void>;
  handleModelSelect: (model: string) => Promise<void>;
  handleModelCreate: (model: string) => Promise<void>;
  handleApiKeyChange: (key: string) => Promise<void>;
  handleBaseUrlChange: (url: string) => Promise<void>;
  handleTestConnection: () => Promise<void>;
};

export const useCloudTranscriptionState = (
  providers: CloudTranscriptionProvider[],
): CloudTranscriptionState => {
  // Читаем из Zustand store (settingsStore хранит AppSettings)
  const settings = useSettingsStore((s) => s.settings);
  const refreshSettings = useSettingsStore((s) => s.refreshSettings);

  const useCloud = settings?.use_cloud_transcription ?? false;
  const selectedProviderId =
    settings?.cloud_transcription_provider_id ?? "groq";

  const selectedProvider = useMemo(
    () => providers.find((p) => p.id === selectedProviderId),
    [providers, selectedProviderId],
  );

  const selectedModel =
    settings?.cloud_transcription_models?.[selectedProviderId] ?? "";
  const apiKey =
    settings?.cloud_transcription_api_keys?.[selectedProviderId] ?? "";
  const baseUrl =
    settings?.cloud_transcription_base_urls?.[selectedProviderId] ??
    selectedProvider?.base_url ??
    "";
  const isCustomProvider = selectedProvider?.allow_base_url_edit ?? false;

  const [testStatus, setTestStatus] = useState<
    "idle" | "testing" | "ok" | "error"
  >("idle");
  const [testError, setTestError] = useState("");

  const handleToggleCloud = useCallback(
    async (enabled: boolean) => {
      await commands.setUseCloudTranscription(enabled);
      await refreshSettings();
    },
    [refreshSettings],
  );

  const handleProviderSelect = useCallback(
    async (id: string) => {
      setTestStatus("idle");
      await commands.setCloudTranscriptionProvider(id);
      await refreshSettings();
    },
    [refreshSettings],
  );

  const handleModelSelect = useCallback(
    async (model: string) => {
      await commands.setCloudTranscriptionModel(selectedProviderId, model);
      await refreshSettings();
    },
    [selectedProviderId, refreshSettings],
  );

  const handleModelCreate = useCallback(
    async (model: string) => {
      await commands.setCloudTranscriptionModel(selectedProviderId, model);
      await refreshSettings();
    },
    [selectedProviderId, refreshSettings],
  );

  const handleApiKeyChange = useCallback(
    async (key: string) => {
      setTestStatus("idle");
      await commands.setCloudTranscriptionApiKey(selectedProviderId, key);
      await refreshSettings();
    },
    [selectedProviderId, refreshSettings],
  );

  const handleBaseUrlChange = useCallback(
    async (url: string) => {
      if (!isCustomProvider) return;
      await commands.setCloudTranscriptionBaseUrl(selectedProviderId, url);
      await refreshSettings();
    },
    [selectedProviderId, isCustomProvider, refreshSettings],
  );

  const handleTestConnection = useCallback(async () => {
    setTestStatus("testing");
    setTestError("");

    try {
      const effectiveBaseUrl = isCustomProvider ? baseUrl : undefined;
      await commands.testCloudTranscriptionProvider(
        selectedProviderId,
        apiKey,
        selectedModel || "whisper-1",
        effectiveBaseUrl ?? null,
      );
      setTestStatus("ok");
    } catch (err) {
      setTestStatus("error");
      setTestError(String(err));
    }
  }, [selectedProviderId, apiKey, selectedModel, baseUrl, isCustomProvider]);

  return {
    providers,
    selectedProviderId,
    selectedProvider,
    selectedModel,
    apiKey,
    baseUrl,
    useCloud,
    isCustomProvider,
    testStatus,
    testError,
    handleToggleCloud,
    handleProviderSelect,
    handleModelSelect,
    handleModelCreate,
    handleApiKeyChange,
    handleBaseUrlChange,
    handleTestConnection,
  };
};
```

### Шаг 6.2: Коммит

```bash
git add src/components/settings/useCloudTranscriptionState.ts
git commit -m "feat: добавить хук useCloudTranscriptionState"
```

---

## Задача 7: UI компонент `CloudTranscriptionSettings.tsx`

**Файлы:**

- Создать: `src/components/settings/CloudTranscriptionSettings.tsx`

По образцу `PostProcessingSettingsApi/PostProcessingSettings.tsx`.

### Шаг 7.1: Создать компонент

```tsx
// src/components/settings/CloudTranscriptionSettings.tsx

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type CloudTranscriptionProvider } from "@/bindings";
import { useCloudTranscriptionState } from "./useCloudTranscriptionState";

export const CloudTranscriptionSettings = () => {
  const { t } = useTranslation();
  const [providers, setProviders] = useState<CloudTranscriptionProvider[]>([]);

  // Загружаем провайдеры один раз при монтировании
  useEffect(() => {
    commands
      .getCloudTranscriptionProviders()
      .then(setProviders)
      .catch(console.error);
  }, []);

  const {
    selectedProviderId,
    selectedProvider,
    selectedModel,
    apiKey,
    baseUrl,
    useCloud,
    isCustomProvider,
    testStatus,
    testError,
    handleToggleCloud,
    handleProviderSelect,
    handleModelSelect,
    handleModelCreate,
    handleApiKeyChange,
    handleBaseUrlChange,
    handleTestConnection,
  } = useCloudTranscriptionState(providers);

  // Опции для дропдауна провайдеров
  const providerOptions = providers.map((p) => ({
    value: p.id,
    label: p.label,
  }));

  // Опции моделей: статические или пустой список для custom
  const modelOptions = (selectedProvider?.static_models ?? []).map((m) => ({
    value: m.id,
    label: m.label,
  }));

  return (
    <div className="flex flex-col gap-4">
      {/* --- Переключатель режима --- */}
      <div className="flex items-center justify-between">
        <div>
          <p className="text-sm font-medium">
            {t("settings.cloudTranscription.title")}
          </p>
          <p className="text-xs text-gray-500">
            {t("settings.cloudTranscription.description")}
          </p>
        </div>
        <button
          onClick={() => handleToggleCloud(!useCloud)}
          className={`relative inline-flex h-6 w-11 rounded-full transition-colors
            ${useCloud ? "bg-blue-500" : "bg-gray-300"}`}
          aria-pressed={useCloud}
        >
          <span
            className={`inline-block h-5 w-5 transform rounded-full bg-white shadow
              transition-transform mt-0.5
              ${useCloud ? "translate-x-5" : "translate-x-0.5"}`}
          />
        </button>
      </div>

      {/* --- Настройки провайдера (показываем только когда включено) --- */}
      {useCloud && (
        <div className="flex flex-col gap-3 rounded-lg border border-gray-200 p-3">
          {/* Выбор провайдера */}
          <div className="flex flex-col gap-1">
            <label className="text-xs font-medium text-gray-600">
              {t("settings.cloudTranscription.provider")}
            </label>
            <select
              value={selectedProviderId}
              onChange={(e) => handleProviderSelect(e.target.value)}
              className="rounded border border-gray-200 bg-white px-2 py-1.5 text-sm"
            >
              {providerOptions.map((opt) => (
                <option key={opt.value} value={opt.value}>
                  {opt.label}
                </option>
              ))}
            </select>
          </div>

          {/* API Ключ */}
          {selectedProvider?.requires_api_key && (
            <div className="flex flex-col gap-1">
              <label className="text-xs font-medium text-gray-600">
                {t("settings.cloudTranscription.apiKey")}
              </label>
              <div className="flex gap-2">
                <input
                  type="password"
                  value={apiKey}
                  onChange={(e) => handleApiKeyChange(e.target.value)}
                  placeholder={t(
                    "settings.cloudTranscription.apiKeyPlaceholder",
                  )}
                  className="flex-1 rounded border border-gray-200 px-2 py-1.5 text-sm font-mono"
                />
                <button
                  onClick={handleTestConnection}
                  disabled={testStatus === "testing" || !apiKey}
                  className="rounded border border-gray-200 px-3 py-1.5 text-xs hover:bg-gray-50
                    disabled:opacity-50"
                >
                  {testStatus === "testing"
                    ? t("settings.cloudTranscription.testing")
                    : t("settings.cloudTranscription.test")}
                </button>
              </div>
              {/* Статус теста */}
              {testStatus === "ok" && (
                <p className="text-xs text-green-600">
                  ✓ {t("settings.cloudTranscription.connected")}
                </p>
              )}
              {testStatus === "error" && (
                <p className="text-xs text-red-500">
                  ✗{" "}
                  {testError ||
                    t("settings.cloudTranscription.connectionFailed")}
                </p>
              )}
            </div>
          )}

          {/* Выбор модели */}
          <div className="flex flex-col gap-1">
            <label className="text-xs font-medium text-gray-600">
              {t("settings.cloudTranscription.model")}
            </label>
            {modelOptions.length > 0 ? (
              <select
                value={selectedModel}
                onChange={(e) => handleModelSelect(e.target.value)}
                className="rounded border border-gray-200 bg-white px-2 py-1.5 text-sm"
              >
                {modelOptions.map((opt) => (
                  <option key={opt.value} value={opt.value}>
                    {opt.label}
                  </option>
                ))}
              </select>
            ) : (
              // Для custom провайдера — свободный ввод
              <input
                type="text"
                value={selectedModel}
                onChange={(e) => handleModelCreate(e.target.value)}
                placeholder="whisper-1"
                className="rounded border border-gray-200 px-2 py-1.5 text-sm"
              />
            )}
          </div>

          {/* Base URL (только для custom) */}
          {isCustomProvider && (
            <div className="flex flex-col gap-1">
              <label className="text-xs font-medium text-gray-600">
                {t("settings.cloudTranscription.baseUrl")}
              </label>
              <input
                type="url"
                value={baseUrl}
                onChange={(e) => handleBaseUrlChange(e.target.value)}
                placeholder="http://localhost:8000/v1"
                className="rounded border border-gray-200 px-2 py-1.5 text-sm font-mono"
              />
            </div>
          )}
        </div>
      )}
    </div>
  );
};
```

### Шаг 7.2: Коммит

```bash
git add src/components/settings/CloudTranscriptionSettings.tsx
git commit -m "feat: добавить CloudTranscriptionSettings UI компонент"
```

---

## Задача 8: Встроить в Settings layout

**Файлы:**

- Найти: файл настроек, который рендерит секцию Transcription/Model (скорее всего `src/components/settings/general/` или аналогичный)
- Изменить: добавить `<CloudTranscriptionSettings />` под секцией выбора модели

### Шаг 8.1: Найти нужный файл

```bash
grep -r "ModelSelector\|ModelUnloadTimeout\|selected_model" \
  /Users/ilyanovik/Documents/Projects/oss/Handy/src/components/settings \
  --include="*.tsx" -l
```

### Шаг 8.2: Добавить импорт и компонент

В найденном файле:

```tsx
import { CloudTranscriptionSettings } from "../CloudTranscriptionSettings";

// В JSX, после секции с выбором модели:
<CloudTranscriptionSettings />;
```

### Шаг 8.3: Коммит

```bash
git add src/components/settings/
git commit -m "feat: встроить CloudTranscriptionSettings в панель настроек"
```

---

## Задача 9: i18n ключи

**Файлы:**

- Изменить: `src/i18n/locales/en/translation.json`
- Изменить: остальные 16 файлов локализации (добавить ключи, оставить пустыми — fallback на en)

### Шаг 9.1: Добавить ключи в английский файл

Найти секцию `"settings"` и добавить в неё:

```json
"cloudTranscription": {
  "title": "Cloud Transcription",
  "description": "Use a cloud API instead of a local model",
  "provider": "Provider",
  "apiKey": "API Key",
  "apiKeyPlaceholder": "Enter your API key",
  "model": "Model",
  "baseUrl": "Base URL",
  "test": "Test",
  "testing": "Testing...",
  "connected": "Connected",
  "connectionFailed": "Connection failed"
}
```

### Шаг 9.2: Добавить заглушки в остальные локали

```bash
# Для каждой локали добавить те же ключи с теми же значениями (en fallback)
for locale in ar cs de es fr it ja ko pl pt ru tr uk vi zh zh-TW; do
  # Добавить cloudTranscription секцию в каждый translation.json
  echo "Обновить: src/i18n/locales/$locale/translation.json"
done
```

Для русской локали (`ru/translation.json`):

```json
"cloudTranscription": {
  "title": "Облачная транскрипция",
  "description": "Использовать облачный API вместо локальной модели",
  "provider": "Провайдер",
  "apiKey": "API ключ",
  "apiKeyPlaceholder": "Введите API ключ",
  "model": "Модель",
  "baseUrl": "Base URL",
  "test": "Тест",
  "testing": "Проверка...",
  "connected": "Подключено",
  "connectionFailed": "Ошибка подключения"
}
```

### Шаг 9.3: Коммит

```bash
git add src/i18n/
git commit -m "feat: добавить i18n ключи для CloudTranscriptionSettings"
```

---

## Задача 10: `has_any_models_available` — учёт облачного режима

**Файлы:**

- Изменить: `src-tauri/src/commands/models.rs`

`has_any_models_available` используется в `App.tsx` для определения, показывать ли онбординг с выбором модели. Если облачный режим включён и API ключ задан — модели "есть".

### Шаг 10.1: Обновить команду

```rust
// В src-tauri/src/commands/models.rs

#[tauri::command]
#[specta::specta]
pub fn has_any_models_available(app: AppHandle) -> bool {
    // НОВОЕ: облачный режим считается "моделью"
    let settings = crate::settings::get_settings(&app);
    if settings.use_cloud_transcription {
        let api_key = settings
            .cloud_transcription_api_keys
            .get(&settings.cloud_transcription_provider_id)
            .map(|k| !k.is_empty())
            .unwrap_or(false);
        // custom провайдер не требует ключ
        let provider_is_custom =
            settings.cloud_transcription_provider_id == "custom";
        if api_key || provider_is_custom {
            return true;
        }
    }

    // Существующая логика — локальные модели
    let model_manager = app.state::<Arc<crate::managers::model::ModelManager>>();
    model_manager.has_any_downloaded_models()
}
```

### Шаг 10.2: Коммит

```bash
git add src-tauri/src/commands/models.rs
git commit -m "fix: has_any_models_available учитывает облачный режим"
```

---

## Задача 11: Финальная проверка

### Шаг 11.1: Полная компиляция бэкенда

```bash
cd /Users/ilyanovik/Documents/Projects/oss/Handy/src-tauri
cargo check
cargo clippy -- -D warnings
```

Ожидаем: 0 ошибок, 0 предупреждений.

### Шаг 11.2: Линтинг фронтенда

```bash
cd /Users/ilyanovik/Documents/Projects/oss/Handy
bun run lint
```

### Шаг 11.3: Запустить в dev-режиме и проверить вручную

```bash
bun run tauri dev
```

**Чеклист ручной проверки:**

- [ ] Settings → открывается секция "Cloud Transcription"
- [ ] Toggle включает облачный режим
- [ ] Выбор Groq → появляются поля API ключ + модель
- [ ] Ввод ключа + кнопка "Test" → возвращает ✓ "Connected"
- [ ] Нажатие горячей клавиши → пишет аудио → отправляет в Groq → вставляет текст
- [ ] Переключение обратно на локальную модель → локальная модель загружается
- [ ] Custom провайдер → появляется поле Base URL
- [ ] Без API ключа test → возвращает понятную ошибку

### Шаг 11.4: Финальный коммит

```bash
git add -p  # проверить что не лишнего
git commit -m "feat: облачная транскрипция через OpenAI-compatible API (Groq, OpenAI, Custom)"
```

---

## Сводная таблица коммитов

| #   | Сообщение коммита                                                        |
| --- | ------------------------------------------------------------------------ |
| 1   | `feat: добавить CloudApiClient и типы провайдеров облачной транскрипции` |
| 2   | `feat: добавить поля cloud_transcription в AppSettings с миграцией`      |
| 3   | `feat: добавить CloudApi вариант в LoadedEngine и load_cloud_provider()` |
| 4   | `feat: добавить Tauri команды для облачной транскрипции`                 |
| 5   | `chore: регенерировать bindings.ts с командами облачной транскрипции`    |
| 6   | `feat: добавить хук useCloudTranscriptionState`                          |
| 7   | `feat: добавить CloudTranscriptionSettings UI компонент`                 |
| 8   | `feat: встроить CloudTranscriptionSettings в панель настроек`            |
| 9   | `feat: добавить i18n ключи для CloudTranscriptionSettings`               |
| 10  | `fix: has_any_models_available учитывает облачный режим`                 |

---

## Известные ограничения / TODO

1. **Translate для Groq**: Groq не поддерживает `task=translate` — в UI нужно отключать переключатель `translate_to_english` когда выбран Groq. Добавить `supports_translation: false` в `CloudTranscriptionProvider` и читать его в компоненте.

2. **Размер файла**: WAV при 16kHz ~32KB/сек. Запись > 12 минут → > 23MB → близко к лимиту 25MB. Следует добавить предупреждение при длинных записях.

3. **Офлайн фоллбэк**: При ошибке сети текущий код просто показывает ошибку. Потенциально можно добавить автоматический fallback на локальную модель.

4. **Хранение API ключей**: Ключи сейчас хранятся в `settings_store.json` (plaintext, аналогично `post_process_api_keys`). Для production: рассмотреть `tauri-plugin-keychain`.

5. **`custom_words` как `prompt`**: Поле `prompt` в Whisper API принимает до ~224 токенов для подсказки словаря. Можно передавать `custom_words.join(", ")` для улучшения точности. Это расширение, не MVP.

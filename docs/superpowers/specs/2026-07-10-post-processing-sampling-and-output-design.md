# Post-processing Sampling and Output Design

## Goal

Give users global temperature and top-k controls for LLM post-processing, and prevent model analysis or reasoning text from being pasted as the processed transcript.

## Settings and interface

- Add `post_process_temperature` as a persisted floating-point setting with a default of `0.2` and an allowed range of `0.0` through `2.0`.
- Add `post_process_top_k` as a persisted integer setting with a default of `40` and an allowed range of `0` through `100`. A value of `0` disables top-k sampling.
- Show both controls in the post-processing settings beside the prompt configuration, following the existing slider and `SettingContainer` patterns.
- Add English source translations for all new user-facing labels and descriptions. Other locales may use the existing i18next fallback until translated.
- Expose dedicated Tauri setting commands and connect them to the Zustand settings updater, preserving the existing optimistic-update behavior.

## Request construction

- Add optional `temperature` and `top_k` fields to the OpenAI-compatible chat-completion request.
- Send temperature to HTTP post-processing providers.
- Send top-k only to the Custom provider because it is not part of the standard OpenAI Chat Completions API and other providers may reject unknown request fields.
- Keep top-k absent when its configured value is `0`.
- Continue using provider-specific reasoning suppression. Custom requests send `reasoning_effort: "none"`; OpenRouter requests send nested reasoning configuration with `effort: "none"` and `exclude: true`.

## Prompt roles and output contract

- Use the selected prompt, with the `${output}` placeholder removed, as a system message for both structured and non-structured HTTP providers.
- Send the raw transcription alone as the user message.
- Append a system-level output contract that prohibits analysis, explanations, drafting notes, preambles, and reasoning, and requires the final transcript to follow a unique marker.
- Structured-output providers continue to use the `transcription` JSON field. Marker extraction is applied to non-structured responses.
- If the expected marker is absent, retain the cleaned response rather than guessing which natural-language paragraphs are commentary. This avoids deleting legitimate dictated content.

## Response sanitization

- Remove complete `<think>...</think>` blocks, including multiline blocks, before returning non-structured content.
- Extract and trim content following the final-output marker when present.
- Continue removing invisible Unicode characters from the final transcript.
- Do not use language-specific heuristics such as removing paragraphs beginning with “The user wants” or “Drafting output.”

The sample response that motivated this work reports zero reasoning tokens and an empty `reasoning_content`; its analysis is ordinary message content. Role separation and the output contract therefore provide the primary fix, while sanitization is a defensive safeguard.

## Validation

- Unit-test request serialization for temperature, disabled/enabled top-k, and reasoning fields.
- Unit-test response cleanup for marker output, multiline `<think>` blocks, missing markers, and ordinary transcript text that contains analysis-like phrases.
- Test settings defaults and persistence-compatible deserialization.
- Run Rust formatting and focused Rust tests, then frontend formatting, linting, and TypeScript/Vite build checks.

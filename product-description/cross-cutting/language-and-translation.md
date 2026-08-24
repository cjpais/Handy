# Language and translation

## Summary

The "Language" picker on the General section records the user's *language intent*: "Auto Detect" or one language. What a dictation actually uses is the *effective language*, worked out fresh for each transcription from the intent and the active model's supported languages and detection capability, and never written back, so a choice made for one model survives a switch to a model that cannot honor it. Around that resolution sit four related behaviors: the picker filtering itself to what the active model can do; "Translate to English", which turns a translation-capable model's transcript into English; a Simplified/Traditional conversion of Chinese output keyed on the effective language; and the output-language *evidence* Handy collects along the way, which decides whether language-specific filler words ("um", "äh", "euh") may be removed. This document owns all of that; [Transcribing](../dictation/transcribing.md) links here for the language step of a dictation and [Models](../foundations/models.md) for the capability words.

## The simple case

With Whisper Medium active, the General section shows a "Whisper Medium Settings" group holding a "Language" row whose chip reads "Auto Detect" and a "Translate to English" toggle, off. The user dictates in Spanish; Whisper detects Spanish and the Spanish text is pasted. The user clicks the chip, types "spa" in the search box, and clicks "Spanish". The chip now reads "Spanish"; the next dictation is transcribed as Spanish without detection, which is a little faster and more accurate for that language. Later the user switches to Parakeet Unified EN 0.6B: the group collapses to nothing, because that model is English only and cannot translate, and every dictation is English. Switching back to Whisper Medium shows "Spanish" again — the intent was kept the whole time.

## The interaction, event by event

The interaction is choosing a language in the picker. It lives in the "{model} Settings" group on General, shown only when the active model has something to configure, and follows the [settings model](../foundations/the-settings-model.md): every choice is written immediately.

```mermaid
stateDiagram-v2
    [*] --> closed
    closed --> open : chip clicked (search box focused, list filtered to the model)
    open --> closed : click outside, or Escape in the search box (no change)
    open --> filtering : text typed in the search box
    filtering --> filtering : more text (list narrows; "No languages found" when empty)
    filtering --> closed : Enter (first match written) or a language clicked (written)
    open --> closed : a language clicked (written)
    closed --> closed : reset arrow (intent back to Auto)
```

### Start

The interaction starts when the user clicks the chip next to "Language" (the ⓘ reveals "Select the language for speech recognition. Auto will automatically determine the language, while selecting a specific language can improve accuracy for that language."). A list drops down with a search box at the top, placeholder "Search languages...", already focused. The list contains only what the active model can use:

- "Auto Detect" appears only if the model can detect languages.
- Every other entry is one of the model's supported languages, by its English name ("English", "German", "Cantonese" …). A model advertising a regional form such as `en-US` still lists "English".
- Chinese is offered as "Chinese (Simplified)" and "Chinese (Traditional)"; there is no plain "Chinese" entry to choose, because all three are recognized identically and only the script of the output differs.

The entry matching the current effective language is highlighted. If the effective language is one the list does not offer — plain Chinese on a Chinese-only model, or a fallback the model forced — nothing is highlighted, though the chip still names it.

### Ends at once

Clicking anywhere outside the list, or pressing Escape in the search box, closes it and clears the search. Nothing is written.

### Becomes active

Typing in the search box narrows the list to names containing the text, case-insensitively. When nothing matches, the list shows "No languages found".

### While active

The list keeps narrowing or widening as the text changes. Hovering highlights an entry. Nothing is written until an entry is chosen.

### Finish

Clicking an entry, or pressing Enter while at least one entry matches (the first match is taken), writes the intent, closes the list, and clears the search. While the write is in flight the row is dimmed with a spinner and the chip is disabled; then the chip shows the chosen name. The chip shows the *effective* language, so choosing a language the model supports shows that name, and an intent the model cannot use would show what the model will actually do instead. The reset arrow beside the chip writes "auto" without opening the list.

> Technical note: the chip's label comes from Handy's language table; if the effective language is a code the table does not know, the chip falls back to "Auto". The highlighted entry and the chip are computed in the settings window from the same rules the transcription pipeline applies, but the pipeline is the authority for the exact code the model receives.

## How the intent resolves

For each transcription Handy computes the effective language from the intent, the active model's supported languages, and whether it can detect:

| Intent | Model supports it | Model can detect | Effective language |
| --- | --- | --- | --- |
| A language | yes | — | that language, in the model's own spelling of it (a bare "en" becomes `en-US` for a model that advertises regional codes) |
| Chinese (Simplified) or (Traditional) | the model has Chinese | — | the script intent itself, so the output conversion below can fire |
| A language | no | yes | Auto |
| Auto | — | yes | Auto |
| A language or Auto | no / — | no | English if the model has it, otherwise the model's first supported language |
| anything | model lists no languages | — | the intent, unchanged |

The result is never saved. Two consequences the user can notice: the chip for a model that cannot honor the intent shows the substitute, not the choice; and switching back to a model that can honor it shows the original choice again.

### What each model does with it

The effective language is a hint. What it means depends on the engine behind the active model:

- **Whisper family** (Whisper Tiny through Large, Breeze-ASR-25, custom `.bin`/`.gguf` Whisper files): a concrete language is passed as a hint and the model transcribes in it; Auto makes the model detect the language from the audio, and that detection becomes the output-language evidence. A hint the loaded model does not list is dropped and the model detects instead. The Custom Words list is also handed to these models as a decoding hint (see [Transcribing](../dictation/transcribing.md)).
- **Parakeet** (the legacy ONNX Parakeet V2 and V3): the hint is ignored entirely. Parakeet V3 detects among its 25 languages no matter what the picker says, and the picker's choice counts for nothing — not even as evidence. Parakeet V2 is English only.
- **SenseVoice** (legacy ONNX): the hint is honored only for Chinese, English, Japanese, Korean, and Cantonese; anything else is Auto. Both Chinese scripts become plain Chinese for recognition.
- **Canary** (legacy ONNX): the model needs an explicit source language and cannot detect, so Auto is never offered; an unusable intent becomes English (or the model's first language). The picker on a Canary model is therefore always a real choice.
- **Cohere Transcribe** (legacy ONNX): a concrete hint is passed; Auto passes nothing and the model detects.
- **Moonshine, GigaAM, MedASR, and every other single-language model:** no hint is passed; the one language is known.
- **Catalog (GGUF) models of any family** run on the same engine as the Whisper family: a hint is passed only if the model lists it, Auto otherwise, and models that can detect report what they detected.

> Technical note: the legacy ONNX engines and the GGUF engine are different code paths for models that may share a name. The catalog "SenseVoice Small" and "Canary 180M Flash" are GGUF models and follow the last bullet; the older `sense-voice-int8` and `canary-180m-flash` directory downloads follow their own bullets. The catalog Cohere Transcribe advertises no detection, so Auto is hidden for it even though the legacy Cohere download offered Auto.

### Translate to English

The "Translate to English" toggle ("Automatically translate speech from other languages to English during transcription.") appears in the "{model} Settings" group only when the active model advertises translation: the multilingual Whisper models except Large v3 Turbo, Breeze-ASR-25, the Canary models, Voxtral Mini 3B and Small 24B, and Granite Speech 4.0 1B and 4.1 2B. The setting is remembered while hidden and silently does nothing for a model that cannot translate.

When it is on and the model can translate, the model is asked to translate rather than transcribe, and the target is always English. Two exceptions:

- **An English source is not translated.** If the effective language is English — chosen, or forced because the model cannot detect — the dictation is a plain transcription. Speaking another language to a model set to English with the toggle on therefore transcribes it as-if-English rather than translating it.
- **Under Auto, the source is what the model detects**; the translation runs on whatever language that is.

The translated text is what is cleaned, saved to history, and pasted; the original-language transcript is not kept. Translation also applies to [live transcription](../dictation/live-transcription.md) with a streaming model that can translate.

> Technical note: the legacy ONNX Canary path differs in one detail: the toggle is passed straight to the engine, which forces English itself, and an English source is not skipped (an English-to-English "translation" runs). The GGUF path skips it.

### Chinese Simplified and Traditional

When the effective language is Chinese (Simplified) or Chinese (Traditional), the transcript is converted to that script after the model runs and before post-processing and pasting: Traditional to Simplified for the former, Simplified to Traditional (Taiwan conventions) for the latter. The conversion is keyed on the *effective* language, so a leftover Chinese-script intent does nothing on a model without Chinese (the intent resolves to Auto or English there), and Japanese kanji from a Japanese dictation are never rewritten. A model whose only language is Chinese (Moonshine Tiny/Base (Chinese)) has no language choice to make except this one, so its picker is shown with just the two Chinese entries and no "Auto Detect". Plain Chinese — the effective language under Auto on such a model, or under an old settings file that stored "zh" — is left in whatever script the model produced.

The converted text is what is pasted and what post-processing receives. In the history entry, however, the model's original-script text is saved as the transcript and the converted text is saved alongside it as the post-processed text, even though no post-processing ran; the History page and "Copy Last Transcript" treat it as such. See [The history page](../history/the-history-page.md).

### Output-language evidence and filler words

Filler-word removal (Advanced › "Remove Filler Words", on by default) has two tiers. Tokens that are not words in any language ("uh", "uhm", "umm", "hmm", "mmm" …) are always removed. Tokens that are real words somewhere — "um", "ah", "eh", "ha" in English; "äh", "ähm" in German; "euh" in French — are removed only when Handy has *evidence* that the output is in that language, because "um" is Portuguese for "a" and German for "around". The evidence, strongest first:

1. **Translated.** The output was translated to English; English fillers apply.
2. **User-selected.** The user chose a language *and* the engine actually received it as a hint. Parakeet V3 ignores hints, so a chosen language is not evidence there.
3. **Model-constrained.** The engine received a language the user did not choose (a forced English, for example), or the model has exactly one language.
4. **Model-detected.** The model detected the language from the audio under Auto (Whisper family, batch or live).
5. **Text-detected.** As a last resort, when none of the above applies, the transcript's own text is examined, constrained to the model's languages, and accepted only if the detector is sure (reliable and at least 0.9 confidence). This catches roughly two thirds of sentences, far more for languages with distinctive scripts than for Latin-script ones, and short texts rarely qualify: "um ok" stays "um ok". It is only attempted when filler removal is on and no custom filler list is set.
6. **Unknown.** Only the universal tier is removed.

A custom filler list (a settings-file-only option with no UI) replaces both tiers and ignores the evidence entirely.

### What the model card shows

Every model card — in onboarding and on the Models page — shows a globe with "{Language} only" (one language; tooltip "Supports this language only") or "{N} languages" (tooltip "Supports multiple input languages"), counting distinct languages after the Chinese scripts are merged; a card with no languages shows no globe. A model that can translate shows "Translate" (tooltip "Can translate to English"). The catalog Whisper Medium reads "99 languages" (the older direct-download Whisper models list 100, Cantonese included), Parakeet V3 "25 languages", Moonshine Base "English only", Moonshine Base (Chinese) "Chinese only". The Models page's "All Languages" filter and "Filter models that support translation to English" toggle use the same data; see [The models page](../models/the-models-page.md).

## Modifiers

| Modifier | Set before the start | Changed while active |
| --- | --- | --- |
| Push to talk | No effect. | No effect. |
| Binding | Transcribe with Post-Processing sends the already-translated and script-converted text to the LLM; the prompt sees the same text the user would otherwise have had pasted. | Fixed at the trigger. |
| Overlay style | Live with a streaming model shows live text in the language (and translation) the stream was started with. | The overlay in use stays. |
| Streaming model | The live stream resolves the effective language, translation, and evidence once when it starts, at the trigger; batch transcription resolves them at the stop. | Fixed at the trigger. |
| Voice activity detection | No effect. | No effect. |
| Always-on microphone | No effect. | No effect. |

## Cancel and interrupt

The columns describe the effect of each event on the language setting and its resolution when no dictation is running and when one is.

| Event | Before a dictation (Handy idle) | During a dictation (recording or processing) |
| --- | --- | --- |
| Cancel | Nothing to cancel; the picker has no cancel of its own beyond closing the list. | A cancel discards the capture or the result; no language work is done or undone. |
| Another trigger | No effect on the setting. | Ignored, as always during a dictation. |
| A setting changed mid-way | Switching the active model re-filters the list and re-resolves the chip immediately; the intent is untouched. Changing Custom Words, filler removal, or the translate toggle takes effect at the next dictation. | Batch: the language, translate toggle, and filler setting are read when the model runs, after the stop. Live: they were read at the trigger. The Chinese conversion reads the effective language again after the model returns, so a language changed during a dictation can make the conversion key off a different language than the transcription ran in. |
| Microphone lost | No effect. | No effect. |
| Model or processing failure | No effect. If the active model is unknown to the model list (a file that disappeared), the intent is passed through as-is and the model decides. | No effect on the setting; a failed transcription has no language step. |
| The active application changes | No effect. | No effect. |
| Handy quits or the system sleeps | The intent and the translate toggle are saved on every change and restored at launch. | The dictation is lost; the settings are unaffected. |
| Keyboard channel changes | No effect. | No effect. |

## Interactions with other systems

**Permissions.** None.

**History and recordings.** The history entry stores the cleaned transcript in the language the model produced (translated to English when translation ran). A Chinese-script conversion is stored as the entry's post-processed text; see above. Re-transcribe from History resolves the language against the active model at that moment, so an entry recorded under Spanish with Whisper can be re-transcribed as English-only with Parakeet.

**Clipboard.** None.

**Model state.** The "{model} Settings" group is hidden when no model is selected or the active model has neither a multi-language choice nor translation. Capabilities are corrected from the real model after its first load, so the group can appear or change after the first dictation with a newly downloaded model.

**Tray and overlay.** None; the tray's model submenu and the overlay show nothing about language.

**Sounds and system audio.** None.

**Settings persistence.** `selected_language` (default "auto") and `translate_to_english` (default off). Both write immediately and both reset from the picker's arrow (language only; the toggle has no reset). The effective language is never written. Filler removal is `filler_word_removal_enabled` (default on) and the hidden `custom_filler_words`.

**Platform differences.** None; the same resolution, translation, conversion, and evidence rules apply on every platform.

## Edge cases

- The chip shows the effective language, not the intent. A user who chose French and then switched to Canary 180M Flash (English, German, Spanish, French) still sees "French"; one who switched to Parakeet Unified EN sees no picker at all; one who switched to Canary 1B v2 (no French) sees "English" and, on switching back, "French" again.
- On the legacy ONNX Parakeet V3 the picker lists 25 languages and accepts a choice that changes nothing in the transcript. The choice is not even used as filler evidence, so "um" survives in an English dictation unless the text detector is confident.
- "Auto Detect" is hidden for Canary, the catalog Cohere, Fun-ASR, Granite, and every single-language model; a stored "auto" intent silently becomes English on them.
- Chinese-only models show a two-entry picker with nothing highlighted until one script is chosen, and the chip reads "Chinese" meanwhile.
- Translate to English with a model set to English does nothing, even if the user speaks Spanish; set the language to Auto or to Spanish first.
- Translate to English is not offered for Whisper Large v3 Turbo, which cannot translate, although the other multilingual Whisper models can.
- A custom Whisper `.gguf` that advertises no languages shows no picker and always detects; its text is still eligible for text-based evidence, unconstrained.
- A model that lists languages the text detector cannot represent (Maltese in Parakeet V3's list, Cantonese in SenseVoice's) still gets detection for the rest; a model whose listed languages are *all* unrepresentable gets none.
- The translate toggle reads the same setting wherever it is shown; turning it on for Whisper and switching to Canary keeps it on.
- Filler removal's language gating is about the output, not the interface language: a German UI does not make "äh" removable from an English transcript.

## Open questions and verification

- Suspected bug: a Chinese Simplified/Traditional conversion is saved in history as *post-processed text* of an entry that was not post-processed, so the History page shows two versions and "Copy Last Transcript" copies the converted one. Probably intentional as a way to keep both scripts, but it mislabels the text and may confuse the History page's post-processing affordances.
- Suspected bug: on the legacy ONNX Parakeet V3 the picker is fully functional but has no effect on transcription (the catalog GGUF build is believed to honor the hint, see below); a user choosing "German" would reasonably expect German output. Either the picker should be hidden for hint-ignoring engines or the choice should at least count as evidence.
- Suspected bug: the legacy Canary path runs translation even when the source is English, unlike the GGUF path; whether the engine treats en→en as a no-op was not verified.
- Whether the chip ever shows the "Auto" fallback label (an effective language outside the language table) was not reproduced; it would require a model advertising a code Handy does not list.
- The exact language count on each card (99 vs 100 for Whisper variants after Chinese scripts are merged) was read from the catalog, not checked in the UI.
- The claim that catalog Parakeet V3 (GGUF) honors a language hint, unlike the legacy ONNX Parakeet V3, follows from the shared GGUF run path; whether the engine actually uses the hint was not tested.
- Text-based detection rates (two thirds of sentences, 99.9% accuracy) come from a calibration comment in the code, not from Handy's own tests.
- The mid-dictation language change producing a conversion keyed on a different language than the transcription ran in was read from the code and not reproduced.

Verified against Handy commit `af48dd6`.

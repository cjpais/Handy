import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, Plus, Trash2 } from "lucide-react";
import type {
  AppSettings,
  AutoSubmitKey,
  PasteMethod,
  ShortcutActivation,
} from "@/bindings";
import { useSettings } from "../../hooks/useSettings";
import { useOsType } from "../../hooks/useOsType";
import { effectiveShortcutSetting } from "../../hooks/useShortcutSetting";
import { ResetButton } from "../ui/ResetButton";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { ShortcutInput } from "./ShortcutInput";
import { ShortcutActivationSetting } from "./ShortcutActivation";
import { PasteMethodSetting } from "./PasteMethod";
import { TypingToolSetting } from "./TypingTool";
import { ClipboardHandlingSetting } from "./ClipboardHandling";
import { AutoSubmit } from "./AutoSubmit";

/** Mirrors `CUSTOM_TRANSCRIBE_PREFIX` in settings.rs. */
const CUSTOM_PREFIX = "transcribe_custom_";
const POST_PROCESS_ID = "transcribe_with_post_process";

const customIndex = (id: string) => Number(id.slice(CUSTOM_PREFIX.length));

/** Built-in transcribe shortcuts first, then user-added ones in order. */
const transcribeBindingIds = (settings: AppSettings | null): string[] => {
  const ids = Object.keys(settings?.bindings ?? {});
  const builtIn = ["transcribe", POST_PROCESS_ID].filter(
    (id) =>
      ids.includes(id) &&
      (id !== POST_PROCESS_ID || settings?.post_process_enabled),
  );
  const custom = ids
    .filter((id) => id.startsWith(CUSTOM_PREFIX))
    .sort((a, b) => customIndex(a) - customIndex(b));
  return [...builtIn, ...custom];
};

const ACTIVATION_LABEL: Record<ShortcutActivation, string> = {
  hold_or_toggle: "settings.general.shortcutActivation.options.holdOrToggle",
  push_to_talk: "settings.general.shortcutActivation.options.pushToTalk",
  toggle: "settings.general.shortcutActivation.options.toggle",
};

const PASTE_LABEL: Record<PasteMethod, string> = {
  ctrl_v: "settings.advanced.pasteMethod.options.clipboard",
  ctrl_shift_v: "settings.advanced.pasteMethod.options.clipboardCtrlShiftV",
  shift_insert: "settings.advanced.pasteMethod.options.clipboardShiftInsert",
  direct: "settings.advanced.pasteMethod.options.direct",
  none: "settings.general.transcribeShortcuts.summary.noPaste",
  external_script: "settings.advanced.pasteMethod.options.externalScript",
};

interface TranscribeShortcutRowProps {
  bindingId: string;
  expanded: boolean;
  onToggle: () => void;
}

/** One transcribe shortcut: its key and a summary, expanding to its settings. */
const TranscribeShortcutRow: React.FC<TranscribeShortcutRowProps> = ({
  bindingId,
  expanded,
  onToggle,
}) => {
  const { t } = useTranslation();
  const osType = useOsType();
  const {
    settings,
    updateBindingOverrides,
    removeTranscribeBinding,
    isUpdating,
  } = useSettings();

  const isCustom = bindingId.startsWith(CUSTOM_PREFIX);
  const postProcessEnabled = settings?.post_process_enabled ?? false;
  const postProcess =
    settings?.bindings?.[bindingId]?.overrides?.post_process ??
    bindingId === POST_PROCESS_ID;

  const activation =
    effectiveShortcutSetting(settings, bindingId, "shortcut_activation") ??
    "hold_or_toggle";
  const pasteMethod =
    effectiveShortcutSetting(settings, bindingId, "paste_method") ?? "ctrl_v";
  const autoSubmit =
    effectiveShortcutSetting(settings, bindingId, "auto_submit") ?? false;
  const autoSubmitKey: AutoSubmitKey =
    effectiveShortcutSetting(settings, bindingId, "auto_submit_key") ?? "enter";

  const submitKeyLabel = {
    enter: t("settings.advanced.autoSubmit.options.enter"),
    ctrl_enter: t("settings.advanced.autoSubmit.options.ctrlEnter"),
    cmd_enter:
      osType === "macos"
        ? t("settings.advanced.autoSubmit.options.cmdEnter")
        : t("settings.advanced.autoSubmit.options.superEnter"),
  }[autoSubmitKey];

  const summary = [
    t(ACTIVATION_LABEL[activation]),
    t(PASTE_LABEL[pasteMethod], {
      modifier: osType === "macos" ? "Cmd" : "Ctrl",
    }),
    autoSubmit
      ? t("settings.general.transcribeShortcuts.summary.submit", {
          key: submitKeyLabel,
        })
      : t("settings.general.transcribeShortcuts.summary.noSubmit"),
    ...(postProcessEnabled && postProcess
      ? [t("settings.general.transcribeShortcuts.summary.postProcess")]
      : []),
  ].join(" · ");

  return (
    <div>
      <ShortcutInput
        shortcutId={bindingId}
        grouped={true}
        title={summary}
        description={t("settings.general.transcribeShortcuts.description")}
        actions={
          <>
            {isCustom && (
              <ResetButton
                onClick={() => removeTranscribeBinding(bindingId)}
                disabled={isUpdating(`binding_${bindingId}`)}
                ariaLabel={t("settings.general.transcribeShortcuts.remove")}
              >
                <Trash2 className="w-4 h-4" />
              </ResetButton>
            )}
            <ResetButton
              onClick={onToggle}
              ariaLabel={t(
                expanded
                  ? "settings.general.transcribeShortcuts.collapse"
                  : "settings.general.transcribeShortcuts.expand",
              )}
            >
              <ChevronDown
                className={`w-4 h-4 transition-transform ${expanded ? "rotate-180" : ""}`}
              />
            </ResetButton>
          </>
        }
      />
      {expanded && (
        <div className="ml-4 mb-2 border-l-2 border-logo-primary/40 divide-y divide-mid-gray/20">
          <ShortcutActivationSetting bindingId={bindingId} grouped={true} />
          {postProcessEnabled && (
            <ToggleSwitch
              checked={postProcess}
              onChange={(enabled) =>
                updateBindingOverrides(bindingId, { post_process: enabled })
              }
              isUpdating={isUpdating(`binding_overrides_${bindingId}`)}
              label={t(
                "settings.general.transcribeShortcuts.postProcess.title",
              )}
              description={t(
                "settings.general.transcribeShortcuts.postProcess.description",
              )}
              grouped={true}
            />
          )}
          <PasteMethodSetting bindingId={bindingId} grouped={true} />
          <TypingToolSetting bindingId={bindingId} grouped={true} />
          <ClipboardHandlingSetting bindingId={bindingId} grouped={true} />
          <AutoSubmit bindingId={bindingId} grouped={true} />
        </div>
      )}
    </div>
  );
};

/**
 * Every transcribe shortcut, each carrying its own activation and output
 * settings, plus a button to add another.
 */
export const TranscribeShortcuts: React.FC = () => {
  const { t } = useTranslation();
  const { settings, addTranscribeBinding } = useSettings();
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [adding, setAdding] = useState(false);

  const toggle = (id: string) =>
    setExpanded((previous) => {
      const next = new Set(previous);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });

  const handleAdd = async () => {
    setAdding(true);
    const id = await addTranscribeBinding();
    setAdding(false);
    // A new shortcut opens so its key and settings can be set right away.
    if (id) {
      setExpanded((previous) => new Set(previous).add(id));
    }
  };

  return (
    <>
      {transcribeBindingIds(settings).map((id) => (
        <TranscribeShortcutRow
          key={id}
          bindingId={id}
          expanded={expanded.has(id)}
          onToggle={() => toggle(id)}
        />
      ))}
      <div className="px-4 p-2">
        <button
          type="button"
          onClick={handleAdd}
          disabled={adding}
          className="flex items-center gap-1 px-2 py-1 text-sm font-medium rounded-md border border-transparent text-text/80 hover:bg-logo-primary/10 hover:border-logo-primary hover:cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <Plus className="w-4 h-4" />
          {t("settings.general.transcribeShortcuts.add")}
        </button>
      </div>
    </>
  );
};

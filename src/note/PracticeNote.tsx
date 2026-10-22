import React, { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { PRACTICE_NOTE_FOCUS_EVENT } from "./events";

/**
 * A blank note to dictate into on onboarding's Try it screen, on Handy's own
 * background so it sits with the window beside it.
 */
export const PracticeNote: React.FC = () => {
  const { t } = useTranslation();
  const ref = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    const report = (on: boolean) => {
      emit(PRACTICE_NOTE_FOCUS_EVENT, { focused: on });
      if (on) ref.current?.focus();
    };
    const unlisten = getCurrentWindow().onFocusChanged(({ payload }) =>
      report(payload),
    );
    // It may have been in front before this listener existed.
    getCurrentWindow()
      .isFocused()
      .then((on) => on && report(true))
      .catch(console.error);
    return () => {
      unlisten.then((off) => off());
    };
  }, []);

  return (
    <div className="h-screen w-full bg-background text-text">
      <textarea
        ref={ref}
        placeholder={t("onboarding.try.notePlaceholder")}
        aria-label={t("onboarding.try.noteLabel")}
        spellCheck={false}
        className="block h-full w-full resize-none border-0 bg-transparent px-[18px] py-[16px] font-[-apple-system,system-ui,'Segoe_UI_Variable_Text','Segoe_UI',sans-serif] text-[15px] leading-[1.55] text-inherit outline-none placeholder:text-ink-3"
      />
    </div>
  );
};

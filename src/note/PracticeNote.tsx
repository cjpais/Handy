import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { emit, listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  PRACTICE_NOTE_FOCUS_EVENT,
  PRACTICE_NOTE_INSERT_EVENT,
} from "./events";

/**
 * A blank note to dictate into on onboarding's Try it screen. It's "another
 * app", so it uses the system's plain colours and font rather than Handy's,
 * and a plum ring says "click here" until it has focus or text.
 */
export const PracticeNote: React.FC = () => {
  const { t } = useTranslation();
  const ref = useRef<HTMLTextAreaElement>(null);
  const [focused, setFocused] = useState(false);
  const [text, setText] = useState("");

  useEffect(() => {
    const report = (on: boolean) => {
      setFocused(on);
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

  // Dictation lands at the cursor, as a paste would.
  useEffect(() => {
    const unlisten = listen<string>(
      PRACTICE_NOTE_INSERT_EVENT,
      ({ payload }) => {
        const area = ref.current;
        if (!area) return;
        area.setRangeText(
          payload,
          area.selectionStart,
          area.selectionEnd,
          "end",
        );
        setText(area.value);
      },
    );
    return () => {
      unlisten.then((off) => off());
    };
  }, []);

  return (
    <div
      className={`h-screen w-full bg-note-surface text-note-ink ${
        focused || text
          ? ""
          : "shadow-[inset_0_0_0_2px_var(--color-logo-stroke)]"
      }`}
    >
      <textarea
        ref={ref}
        value={text}
        onChange={(e) => setText(e.target.value)}
        placeholder={t("onboarding.try.notePlaceholder")}
        aria-label={t("onboarding.try.noteLabel")}
        spellCheck={false}
        className="block h-full w-full resize-none border-0 bg-transparent px-[18px] py-[16px] font-[-apple-system,system-ui,'Segoe_UI_Variable_Text','Segoe_UI',sans-serif] text-[15px] leading-[1.55] text-inherit outline-none placeholder:text-[rgba(127,127,127,0.8)]"
      />
    </div>
  );
};

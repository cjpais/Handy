import { getCurrentWindow } from "@tauri-apps/api/window";
import React, { useEffect, useMemo, useRef, useState } from "react";
import "./PromptPicker.css";
import { commands, events } from "@/bindings";
import type { LLMPrompt } from "@/bindings";

const PromptPicker: React.FC = () => {
  const [prompts, setPrompts] = useState<LLMPrompt[]>([]);
  const [lastUsedPromptId, setLastUsedPromptId] = useState<string | null>(
    null,
  );
  const [selectedIndex, setSelectedIndex] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);
  // The window reopens at the same screen position each time, so a mouse
  // left resting over item 3 fires a stale `mouseenter` on reopen and
  // silently overrides the keyboard-driven reset to the top item. Hover
  // selection is only armed once the mouse actually moves after showing.
  const mouseArmedRef = useRef(false);

  // Most recently used prompt floats to the top; everything else keeps its order.
  const displayList = useMemo(() => {
    const idx = prompts.findIndex((p) => p.id === lastUsedPromptId);
    if (idx <= 0) return prompts;
    const reordered = [...prompts];
    const [lastUsed] = reordered.splice(idx, 1);
    reordered.unshift(lastUsed);
    return reordered;
  }, [prompts, lastUsedPromptId]);

  useEffect(() => {
    const unlistenPromise = events.promptPickerShowEvent.listen((event) => {
      setPrompts(event.payload.prompts);
      setLastUsedPromptId(event.payload.last_used_prompt_id ?? null);
      setSelectedIndex(0);
      mouseArmedRef.current = false;
    });

    const unlistenFocusPromise = getCurrentWindow().onFocusChanged(
      ({ payload: focused }) => {
        if (!focused) commands.cancelPromptChoice();
      },
    );

    const onMouseMove = () => {
      mouseArmedRef.current = true;
    };
    window.addEventListener("mousemove", onMouseMove);

    return () => {
      unlistenPromise.then((unlisten) => unlisten());
      unlistenFocusPromise.then((unlisten) => unlisten());
      window.removeEventListener("mousemove", onMouseMove);
    };
  }, []);

  useEffect(() => {
    listRef.current
      ?.querySelector(`[data-index="${selectedIndex}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [selectedIndex]);

  const confirm = (index: number) => {
    const prompt = displayList[index];
    if (prompt) commands.submitPromptChoice(prompt.id);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      commands.cancelPromptChoice();
      return;
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelectedIndex((i) => (i + 1) % displayList.length);
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelectedIndex(
        (i) => (i - 1 + displayList.length) % displayList.length,
      );
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      confirm(selectedIndex);
      return;
    }
    const digit = Number(e.key);
    if (Number.isInteger(digit) && digit >= 1 && digit <= 9) {
      const index = digit - 1;
      if (index < displayList.length) {
        e.preventDefault();
        confirm(index);
      }
    }
  };

  const selectedPrompt = displayList[selectedIndex];

  return (
    <div className="pp-container" tabIndex={0} autoFocus onKeyDown={handleKeyDown}>
      <div className="pp-body">
        <div className="pp-list" ref={listRef}>
          {displayList.map((prompt, index) => (
            <div
              key={`${prompt.id}-${index}`}
              data-index={index}
              className={`pp-item ${index === selectedIndex ? "selected" : ""}`}
              onMouseEnter={() => {
                if (mouseArmedRef.current) setSelectedIndex(index);
              }}
              onClick={() => confirm(index)}
            >
              {prompt.name}
            </div>
          ))}
        </div>
        <div className="pp-preview">{selectedPrompt?.prompt}</div>
      </div>
      <div className="pp-footer">
        <span>Prompt library</span>
        <span>Paste ⏎</span>
      </div>
    </div>
  );
};

export default PromptPicker;

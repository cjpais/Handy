import React, { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Globe } from "lucide-react";
import { Button } from "../../ui/Button";
import { Radio } from "../../ui/Radio";
import { Mascot } from "../../shared/Mascot";
import HandyTextLogo from "../../icons/HandyTextLogo";
import { OnboardingShell, ScreenSub, ScreenTitle } from "../OnboardingShell";
import { useOnboardingStore } from "../onboardingStore";
import {
  CHINESE_SCRIPTS,
  isChinese,
  languageName,
  searchLanguages,
  spokenLanguage,
} from "../data/languages";
import { CURATED_ASR, isWeakLanguage } from "../data/models";
import { modelMatchesLanguage } from "@handy-computer/models";
import { SUPPORTED_LANGUAGES, getSupportedLanguage } from "@/i18n";
import { useSettings } from "@/hooks/useSettings";

// The mascot waves on the first visit only.
let waved = false;

const RemoveIcon = () => (
  <svg
    width="10"
    height="10"
    viewBox="0 0 10 10"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.6"
    strokeLinecap="round"
    aria-hidden="true"
  >
    <path d="M2 2l6 6M8 2L2 8" />
  </svg>
);

interface WelcomeProps {
  progress: { index: number; count: number };
  /** Changing the menu language writes the setting (not in a preview). */
  preview: boolean;
  onContinue: () => void;
}

/**
 * Screen 1: the greeting, the languages you speak, and which Chinese
 * characters Handy writes when you speak Chinese or Cantonese.
 */
export const Welcome: React.FC<WelcomeProps> = ({
  progress,
  preview,
  onContinue,
}) => {
  const { t, i18n } = useTranslation();
  const { updateSetting } = useSettings();
  const { languages, script, modelId, set } = useOnboardingStore();
  const [query, setQuery] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const [wave] = useState(() => !waved);
  useEffect(() => {
    waved = true;
    const id = setTimeout(
      () => inputRef.current?.focus({ preventScroll: true }),
      50,
    );
    return () => clearTimeout(id);
  }, []);

  const ui = i18n.language;
  const nameOf = (code: string) => languageName(code, ui);
  const listOf = (codes: string[]) =>
    new Intl.ListFormat(ui, { type: "conjunction" }).format(codes.map(nameOf));

  const toggle = (code: string) => {
    const next = languages.includes(code)
      ? languages.filter((c) => c !== code)
      : [...languages, code];
    // A model picked for other languages no longer counts as the pick.
    const picked = CURATED_ASR.find((m) => m.id === modelId);
    const stillFits =
      !picked || next.every((l) => modelMatchesLanguage(picked, l));
    set({
      languages: next,
      ...(stillFits ? {} : { modelId: null }),
    });
    setQuery("");
    inputRef.current?.focus({ preventScroll: true });
  };

  const matches = useMemo(() => searchLanguages(query, ui), [query, ui]);
  const available = matches.filter((l) => !languages.includes(l.code));
  const chinese = languages.filter(isChinese);
  const weak = languages.filter(isWeakLanguage);

  const onKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.nativeEvent.isComposing) return;
    if (e.key === "Enter" && query) {
      e.preventDefault();
      if (available[0]) toggle(available[0].code);
    } else if (e.key === "Backspace" && !query && languages.length) {
      e.preventDefault();
      toggle(languages[languages.length - 1]);
    } else if (e.key === "Escape" && query) {
      setQuery("");
    }
  };

  const hint = !languages.length
    ? t("onboarding.welcome.hint.pickOne")
    : weak.length
      ? t("onboarding.welcome.hint.weak", {
          count: weak.length,
          languages: listOf(weak),
        })
      : "";

  // What the menus are in right now (a preview never writes the setting).
  const uiLanguage = getSupportedLanguage(ui) || "en";
  const changeUiLanguage = (code: string) => {
    i18n.changeLanguage(code);
    if (!preview) updateSetting("app_language", code);
  };

  return (
    <OnboardingShell
      progress={progress}
      left={
        <label
          title={t("onboarding.welcome.appLanguage")}
          className="flex shrink-0 items-center gap-[6px]"
        >
          <Globe size={16} aria-hidden="true" className="text-ink-3" />
          <select
            aria-label={t("onboarding.welcome.appLanguage")}
            value={uiLanguage}
            onChange={(e) => changeUiLanguage(e.target.value)}
            className="rounded-[7px] border border-line bg-transparent px-[6px] py-[3px] text-small text-text"
          >
            {SUPPORTED_LANGUAGES.map((l) => (
              <option key={l.code} value={l.code} lang={l.code}>
                {l.nativeName}
              </option>
            ))}
          </select>
        </label>
      }
      hint={hint}
      hintTone={languages.length && weak.length ? "warning" : "quiet"}
      primary={
        <Button
          variant="primary"
          size="lg"
          disabled={!languages.length}
          onClick={onContinue}
        >
          {t("onboarding.shell.continue")}
        </Button>
      }
    >
      <div className="flex h-full min-h-0 flex-col items-center px-gutter pt-[6px] pb-[4px] text-center">
        <div className="mb-[10px] inline-flex items-end gap-[8px] leading-none">
          <Mascot size={54} animation={wave ? "wave" : null} />
          <HandyTextLogo width={112} className="mb-[4px] block" />
        </div>
        <ScreenTitle>{t("onboarding.welcome.title")}</ScreenTitle>
        <ScreenSub className="mt-[4px]! max-w-none!">
          {t("onboarding.welcome.subtitle")}
        </ScreenSub>

        <div className="mt-[18px] flex min-h-0 w-full flex-1 flex-col overflow-hidden rounded-panel border-[1.5px] border-line-strong bg-background">
          {/* Clicks around the chips go to the search (a <label> would hand
              them to the first chip, which removes it). */}
          <div
            onClick={() => inputRef.current?.focus({ preventScroll: true })}
            className="flex min-h-[46px] cursor-text flex-wrap items-center gap-[4px] border-b border-line px-[8px] py-[7px] text-start"
          >
            {languages.map((code) => (
              <button
                key={code}
                type="button"
                aria-label={t("onboarding.welcome.remove", {
                  language: nameOf(code),
                })}
                onMouseDown={(e) => e.preventDefault()}
                onClick={(e) => {
                  e.preventDefault();
                  toggle(code);
                }}
                className="group inline-flex h-[28px] cursor-pointer items-center gap-[6px] rounded-full border-[1.5px] border-line-strong bg-background ps-[12px] pe-[6px] text-small font-semibold text-text hover:border-[color-mix(in_srgb,var(--color-logo-stroke)_40%,var(--color-line-strong))]"
              >
                <bdi lang={code}>{spokenLanguage(code)?.native ?? code}</bdi>
                <span className="grid size-[18px] place-items-center rounded-full leading-none text-ink-2 group-hover:bg-[color-mix(in_srgb,var(--color-text)_10%,transparent)]">
                  <RemoveIcon />
                </span>
              </button>
            ))}
            <input
              ref={inputRef}
              type="text"
              autoComplete="off"
              spellCheck={false}
              aria-label={t("onboarding.welcome.searchLabel")}
              placeholder={
                languages.length
                  ? t("onboarding.welcome.addAnother")
                  : t("onboarding.welcome.searchPlaceholder")
              }
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={onKeyDown}
              className="h-[30px] min-w-[120px] flex-1 border-0 bg-transparent px-[8px] text-body text-text outline-none placeholder:text-ink-3"
            />
          </div>
          <div
            role="group"
            aria-label={t("onboarding.welcome.languagesLabel")}
            className="grid min-h-0 flex-1 grid-cols-3 content-start gap-[2px] overflow-y-auto p-[6px] pb-[14px] text-start [scrollbar-width:thin] [mask-image:linear-gradient(to_bottom,black_calc(100%-40px),transparent_calc(100%-6px))]"
          >
            {available.map((l, i) => {
              const name = nameOf(l.code);
              const first = !!query && i === 0;
              return (
                <button
                  key={l.code}
                  type="button"
                  onClick={() => toggle(l.code)}
                  className={`flex h-[46px] cursor-pointer flex-col justify-center rounded-[9px] px-[12px] text-start leading-[1.25] hover:bg-wash ${
                    first
                      ? "border-[1.5px] border-line-strong bg-wash"
                      : "border border-transparent"
                  }`}
                >
                  <b lang={l.code} className="font-semibold">
                    {l.native}
                  </b>
                  <small className="text-label text-ink-3">
                    {l.native !== name ? name : " "}
                  </small>
                </button>
              );
            })}
            {!available.length && (
              <div className="col-span-full p-[24px] text-center text-small text-ink-3">
                {matches.length
                  ? t("onboarding.welcome.alreadyAdded", {
                      count: matches.length,
                      languages: listOf(matches.map((l) => l.code)),
                    })
                  : t("onboarding.welcome.noMatch", { query })}
              </div>
            )}
          </div>
        </div>

        {chinese.length > 0 && (
          <div className="w-full animate-rise pt-[12px] text-start">
            <p className="mt-0 mb-[8px] text-small font-semibold text-text">
              {t("onboarding.welcome.chinese.question", {
                languages: listOf(chinese),
              })}
            </p>
            <div
              role="radiogroup"
              aria-label={t("onboarding.welcome.chinese.label")}
              className="flex flex-wrap gap-[8px]"
            >
              {CHINESE_SCRIPTS.map((s) => {
                const picked = script === s.id;
                return (
                  <button
                    key={s.id}
                    type="button"
                    role="radio"
                    aria-checked={picked}
                    onClick={() => set({ script: s.id })}
                    className={`inline-flex h-[34px] cursor-pointer items-center gap-[8px] rounded-control border-[1.5px] bg-background ps-[10px] pe-[14px] text-small font-semibold text-text ${
                      picked
                        ? "border-plum-pick"
                        : "border-line-strong hover:border-[color-mix(in_srgb,var(--color-logo-stroke)_40%,var(--color-line-strong))]"
                    }`}
                  >
                    <Radio checked={picked} />
                    <span>
                      <span lang={s.lang}>{s.native}</span>{" "}
                      {t(`onboarding.welcome.chinese.${s.id}`)}
                    </span>
                  </button>
                );
              })}
            </div>
          </div>
        )}
      </div>
    </OnboardingShell>
  );
};

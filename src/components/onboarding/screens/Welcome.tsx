import React, { useEffect, useMemo, useRef, useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import { ChevronDown, Globe } from "lucide-react";
import { Button } from "../../ui/Button";
import { Radio } from "../../ui/Radio";
import { Mascot } from "../../shared/Mascot";
import {
  LanguageChip,
  LanguageOption,
  languageSearchKeys,
} from "../../shared/LanguagePicker";
import HandyTextLogo from "../../icons/HandyTextLogo";
import { OnboardingShell, ScreenHeader } from "../OnboardingShell";
import { useOnboardingStore } from "../onboardingStore";
import { CHINESE_SCRIPTS, isChinese } from "../data/languages";
import { CURATED_ASR, isWeakLanguage } from "../data/models";
import { modelMatchesLanguage } from "@handy-computer/models";
import { SUPPORTED_LANGUAGES, getSupportedLanguage } from "@/i18n";
import {
  languageList,
  languageName,
  searchLanguages,
} from "@/lib/constants/languages";
import { useSettings } from "@/hooks/useSettings";

// The mascot waves on the first visit only.
let waved = false;

interface WelcomeProps {
  progress: { index: number; count: number };
  onContinue: () => void;
}

/** Screen 1: greeting, spoken languages, and Chinese script. */
export const Welcome: React.FC<WelcomeProps> = ({ progress, onContinue }) => {
  const { t, i18n } = useTranslation();
  const { updateSetting } = useSettings();
  const { languages, script, modelId, preview, set } = useOnboardingStore();
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

  const onKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) =>
    languageSearchKeys(e, {
      query,
      first: available[0]?.code,
      last: languages[languages.length - 1],
      toggle,
      clear: () => setQuery(""),
    });

  const hint = !languages.length
    ? t("onboarding.welcome.hint.pickOne")
    : weak.length
      ? t("onboarding.welcome.hint.weak", {
          count: weak.length,
          languages: languageList(weak, ui),
        })
      : "";

  // What the menus are in right now (a preview never writes the setting).
  const uiLanguage = getSupportedLanguage(ui) || "en";
  const uiLanguageName =
    SUPPORTED_LANGUAGES.find((l) => l.code === uiLanguage)?.nativeName ??
    uiLanguage;
  const changeUiLanguage = (code: string) => {
    i18n.changeLanguage(code);
    if (!preview) updateSetting("app_language", code);
  };

  return (
    <OnboardingShell
      progress={progress}
      pill={
        // App language: a native <select> laid invisibly over the label.
        <label
          title={t("onboarding.welcome.appLanguage")}
          className="relative -me-[8px] flex shrink-0 cursor-pointer items-center gap-[6px] rounded-lg px-[8px] py-[4px] text-small text-ink-2 transition-colors hover:bg-wash hover:text-text has-[select:focus-visible]:bg-wash has-[select:focus-visible]:text-text"
        >
          <Globe size={16} aria-hidden="true" />
          <span>
            <Trans
              i18nKey="onboarding.welcome.menusIn"
              values={{ language: uiLanguageName }}
              components={{ b: <b className="font-semibold text-text" /> }}
            />
          </span>
          <ChevronDown size={14} aria-hidden="true" />
          <select
            aria-label={t("onboarding.welcome.appLanguage")}
            value={uiLanguage}
            onChange={(e) => changeUiLanguage(e.target.value)}
            className="absolute inset-0 cursor-pointer opacity-0"
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
      hintTone={weak.length ? "warning" : "quiet"}
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
        <ScreenHeader
          title={t("onboarding.welcome.title")}
          sub={t("onboarding.welcome.subtitle")}
        />

        <div className="mt-lead flex min-h-0 w-full flex-1 flex-col overflow-hidden rounded-panel border-[1.5px] border-line-strong bg-background">
          {/* Clicks around the chips go to the search (a <label> would hand
              them to the first chip, which removes it). */}
          <div
            onClick={() => inputRef.current?.focus({ preventScroll: true })}
            className="flex min-h-[46px] cursor-text flex-wrap items-center gap-[4px] border-b border-line px-[8px] py-[7px] text-start"
          >
            {languages.map((code) => (
              <LanguageChip
                key={code}
                code={code}
                removeLabel={t("onboarding.welcome.remove", {
                  language: nameOf(code),
                })}
                onRemove={() => toggle(code)}
              />
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
            {available.map((l, i) => (
              <LanguageOption
                key={l.code}
                language={l}
                name={nameOf(l.code)}
                highlighted={!!query && i === 0}
                onPick={() => toggle(l.code)}
              />
            ))}
            {!available.length && (
              <div className="col-span-full p-[24px] text-center text-small text-ink-3">
                {matches.length
                  ? t("onboarding.welcome.alreadyAdded", {
                      count: matches.length,
                      languages: languageList(
                        matches.map((l) => l.code),
                        ui,
                      ),
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
                languages: languageList(chinese, ui),
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
                        : "border-line-strong hover:border-ink-3"
                    }`}
                  >
                    <Radio checked={picked} />
                    <span>
                      <span lang={s.lang} className="font-cjk-subset">
                        {s.native}
                      </span>{" "}
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

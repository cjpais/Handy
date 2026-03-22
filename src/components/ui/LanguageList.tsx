import React, { useState, useRef, useEffect } from "react";
import { Globe, ChevronDown } from "lucide-react";
import { LANGUAGES } from "../../lib/constants/languages";
import { useTranslation } from "react-i18next";

interface LanguageListProps {
  languages: string[];
  className?: string;
  maxDisplay?: number;
  align?: "left" | "right";
}

export const LanguageList: React.FC<LanguageListProps> = ({
  languages,
  className = "",
  maxDisplay = 3,
  align = "left",
}) => {
  const { t } = useTranslation();
  const [isOpen, setIsOpen] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (
        dropdownRef.current &&
        !dropdownRef.current.contains(event.target as Node)
      ) {
        setIsOpen(false);
      }
    };
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, []);

  if (!languages || languages.length === 0) return null;

  const mappedLanguages = languages.map(
    (lang) => LANGUAGES.find((l) => l.value === lang)?.label || lang,
  );

  if (languages.length <= maxDisplay) {
    return (
      <div
        className={`flex items-center gap-1.5 text-xs text-text/50 ${className}`}
      >
        <Globe className="w-3.5 h-3.5 shrink-0" />
        <span className="truncate">{mappedLanguages.join(", ")}</span>
      </div>
    );
  }

  const displayed = mappedLanguages.slice(0, maxDisplay);
  const remaining = languages.length - displayed.length;

  return (
    <div className={`relative ${className}`} ref={dropdownRef}>
      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          setIsOpen(!isOpen);
        }}
        className="flex items-center gap-1.5 text-xs font-medium text-text/50 hover:text-logo-primary transition-colors py-0.5 px-1.5 -mx-1.5 rounded-lg hover:bg-logo-primary/5 focus:outline-none focus:ring-2 focus:ring-logo-primary/20"
      >
        <Globe className="w-3.5 h-3.5 shrink-0" />
        <span className="truncate">{displayed.join(", ")}</span>
        {remaining > 0 && (
          <span className="bg-mid-gray/10 px-1.5 py-0.5 rounded text-[10px] font-bold text-text/40 group-hover:text-logo-primary transition-colors whitespace-nowrap">
            +{remaining} {t("common.more", "more")}
          </span>
        )}
        <ChevronDown
          className={`w-3 h-3 transition-transform duration-200 shrink-0 ${isOpen ? "rotate-180" : ""}`}
        />
      </button>

      {isOpen && (
        <div
          className={`absolute bottom-full mb-2 w-48 bg-background border border-mid-gray/80 rounded-xl shadow-xl z-[60] overflow-hidden ${align === "right" ? "right-0" : "left-0"}`}
          onClick={(e) => e.stopPropagation()}
        >
          <div className="max-h-60 overflow-y-auto py-1.5">
            <div className="px-3 py-1 text-[10px] font-bold uppercase tracking-wider text-text/30 border-b border-mid-gray/10 mb-1">
              {t(
                "modelSelector.capabilities.supportedLanguages",
                "Supported Languages",
              )}
            </div>
            {mappedLanguages.map((lang, i) => (
              <div
                key={i}
                className="px-3 py-1.5 text-xs text-text/80 hover:bg-mid-gray/5 transition-colors"
              >
                {lang}
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
};

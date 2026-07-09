import React, { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import type { ModelInfo } from "@/bindings";
import {
  getTranslatedModelName,
  getTranslatedModelDescription,
} from "../../lib/utils/modelTranslation";
import {
  VIEWPORT_PADDING,
  clampHorizontal,
  resolveVerticalPosition,
} from "../../lib/utils/viewportPosition";

interface ModelDropdownProps {
  models: ModelInfo[];
  currentModelId: string;
  onModelSelect: (modelId: string) => void;
  targetRef: React.RefObject<HTMLElement | null>;
  onClose: () => void;
}

const DROPDOWN_WIDTH = 256; // matches w-64
const DEFAULT_HEIGHT = 200;
const GAP = 8;

interface DropdownCoords {
  top: number;
  left: number;
}

const ModelDropdown: React.FC<ModelDropdownProps> = ({
  models,
  currentModelId,
  onModelSelect,
  targetRef,
  onClose,
}) => {
  const { t } = useTranslation();
  const downloadedModels = models.filter((m) => m.is_downloaded);
  const contentRef = useRef<HTMLDivElement>(null);
  const [coords, setCoords] = useState<DropdownCoords | null>(null);

  const updatePosition = useCallback(() => {
    if (!targetRef.current) return;

    const targetRect = targetRef.current.getBoundingClientRect();
    const contentHeight = contentRef.current?.offsetHeight || DEFAULT_HEIGHT;

    const { top } = resolveVerticalPosition(
      targetRect,
      contentHeight,
      "top",
      GAP,
    );

    const isRtl = document.documentElement.dir === "rtl";
    const start = isRtl ? targetRect.right - DROPDOWN_WIDTH : targetRect.left;
    const left = clampHorizontal(start, DROPDOWN_WIDTH, VIEWPORT_PADDING);

    setCoords({ top, left });
  }, [targetRef]);

  useEffect(() => {
    updatePosition();
    window.addEventListener("scroll", updatePosition, true);
    window.addEventListener("resize", updatePosition);
    return () => {
      window.removeEventListener("scroll", updatePosition, true);
      window.removeEventListener("resize", updatePosition);
    };
  }, [updatePosition]);

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      const target = event.target as Node;
      if (
        contentRef.current?.contains(target) ||
        targetRef.current?.contains(target)
      ) {
        return;
      }
      onClose();
    };

    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [onClose, targetRef]);

  const handleModelClick = (modelId: string) => {
    onModelSelect(modelId);
  };

  return createPortal(
    <div
      ref={contentRef}
      style={{
        position: "fixed",
        top: coords?.top ?? -9999,
        left: coords?.left ?? -9999,
        width: DROPDOWN_WIDTH,
        opacity: coords ? 1 : 0,
      }}
      className="max-h-[60vh] overflow-y-auto bg-background border border-mid-gray/20 rounded-lg shadow-lg py-2 z-50"
    >
      {downloadedModels.length > 0 ? (
        <div>
          {downloadedModels.map((model) => (
            <div
              key={model.id}
              onClick={() => handleModelClick(model.id)}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  handleModelClick(model.id);
                }
              }}
              tabIndex={0}
              role="button"
              className={`w-full px-3 py-2 text-start hover:bg-mid-gray/10 transition-colors cursor-pointer focus:outline-none ${
                currentModelId === model.id
                  ? "bg-logo-primary/10 text-logo-primary"
                  : ""
              }`}
            >
              <div className="flex items-center justify-between">
                <div>
                  <div className="text-sm text-text/80">
                    {getTranslatedModelName(model, t)}
                    {model.is_custom && (
                      <span className="ms-1.5 text-[10px] font-medium text-text/40 uppercase">
                        {t("modelSelector.custom")}
                      </span>
                    )}
                  </div>
                  <div className="text-xs text-text/40 italic pe-4">
                    {getTranslatedModelDescription(model, t)}
                  </div>
                </div>
                {currentModelId === model.id && (
                  <div className="text-xs text-logo-primary">
                    {t("modelSelector.active")}
                  </div>
                )}
              </div>
            </div>
          ))}
        </div>
      ) : (
        <div className="px-3 py-2 text-sm text-text/60">
          {t("modelSelector.noModelsAvailable")}
        </div>
      )}
    </div>,
    document.body,
  );
};

export default ModelDropdown;

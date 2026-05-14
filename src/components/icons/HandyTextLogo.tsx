import React from "react";
import { useTranslation } from "react-i18next";
import unburdnIconLogo from "../../assets/unburdn-icon-logo.png";

const HandyTextLogo = ({
  width,
  height,
  className,
}: {
  width?: number;
  height?: number;
  className?: string;
}) => {
  const { t } = useTranslation();
  const resolvedHeight = height ?? (width ? Math.min(width, 64) : undefined);

  return (
    <img
      alt={t("brand.name")}
      className={`block object-contain ${className ?? ""}`}
      src={unburdnIconLogo}
      style={{ width, height: resolvedHeight }}
    />
  );
};

export default HandyTextLogo;

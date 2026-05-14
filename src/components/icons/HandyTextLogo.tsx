import React from "react";
import { useTranslation } from "react-i18next";

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

  return (
    <div
      className={`font-semibold tracking-normal text-text ${className ?? ""}`}
      style={{ width, height }}
    >
      {t("brand.name")}
    </div>
  );
};

export default HandyTextLogo;

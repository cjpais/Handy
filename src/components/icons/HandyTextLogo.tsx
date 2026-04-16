/* eslint-disable i18next/no-literal-string */
import React from "react";

const HandyTextLogo = ({
  width,
  height,
  className,
}: {
  width?: number;
  height?: number;
  className?: string;
}) => {
  return (
    <svg
      width={width}
      height={height}
      className={className}
      viewBox="0 0 300 60"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
    >
      <text
        x="150"
        y="45"
        textAnchor="middle"
        className="fill-logo-primary"
        style={{
          fontSize: "48px",
          fontWeight: 700,
          fontFamily: "system-ui, -apple-system, sans-serif",
          letterSpacing: "-0.02em",
        }}
      >
        Uspeach
      </text>
    </svg>
  );
};

export default HandyTextLogo;

/* eslint-disable i18next/no-literal-string */
import React from "react";

const ThegAiTextLogo = ({
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
      viewBox="0 0 400 120"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
    >
      <text
        x="10"
        y="85"
        fontFamily="system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif"
        fontWeight="800"
        fontSize="80"
        fill="currentColor"
        letterSpacing="-3"
      >
        Theg
        <tspan fill="var(--color-terracotta, #e05a47)">Ai</tspan>
      </text>
    </svg>
  );
};

export default ThegAiTextLogo;

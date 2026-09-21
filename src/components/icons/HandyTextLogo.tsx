import React from "react";

// Brand name, intentionally not translated.
const WORDMARK = "Tolkovo";

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
      viewBox="0 0 930 328"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
    >
      {/* Tolkovo: plain text wordmark until a dedicated logo exists. */}
      <text
        x="465"
        y="164"
        textAnchor="middle"
        dominantBaseline="central"
        fontSize="220"
        fontWeight="700"
        fontFamily="-apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif"
        className="logo-primary"
      >
        {WORDMARK}
      </text>
    </svg>
  );
};

export default HandyTextLogo;

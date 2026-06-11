import React from "react";

const BRAND_NAME = "ixiwhisper";
const BRAND_PREFIX = "ixi";
const BRAND_SUFFIX = "whisper";

const HandyTextLogo = ({
  width,
  height,
  className,
}: {
  width?: number;
  height?: number;
  className?: string;
}) => {
  const resolvedHeight = height ?? (width ? (width * 160) / 570 : undefined);

  return (
    <svg
      width={width}
      height={resolvedHeight}
      className={className}
      viewBox="0 0 570 160"
      fill="none"
      role="img"
      aria-label={BRAND_NAME}
      xmlns="http://www.w3.org/2000/svg"
    >
      <defs>
        <linearGradient
          id="ixiwhisper-wordmark-gradient"
          x1="0"
          y1="28"
          x2="152"
          y2="122"
          gradientUnits="userSpaceOnUse"
        >
          <stop stopColor="#ff9a1f" />
          <stop offset="0.54" stopColor="#ff6d00" />
          <stop offset="1" stopColor="#f04d00" />
        </linearGradient>
      </defs>
      <text
        x="10"
        y="108"
        className="wordmark-shadow"
        fontFamily="'Avenir Next Rounded', 'Avenir Next', 'Nunito Sans', ui-rounded, 'SF Pro Rounded', Inter, system-ui, sans-serif"
        fontSize="82"
        fontWeight="900"
      >
        {BRAND_PREFIX}
      </text>
      <text
        x="148"
        y="108"
        className="wordmark-shadow"
        fontFamily="'Avenir Next Rounded', 'Avenir Next', 'Nunito Sans', ui-rounded, 'SF Pro Rounded', Inter, system-ui, sans-serif"
        fontSize="82"
        fontWeight="800"
      >
        {BRAND_SUFFIX}
      </text>
      <g className="wordmark-wave" aria-hidden="true">
        <rect x="22" y="116" width="8" height="18" rx="4" />
        <rect x="38" y="108" width="8" height="34" rx="4" />
        <rect x="54" y="120" width="8" height="14" rx="4" />
      </g>
      <text
        x="10"
        y="106"
        fill="url(#ixiwhisper-wordmark-gradient)"
        fontFamily="'Avenir Next Rounded', 'Avenir Next', 'Nunito Sans', ui-rounded, 'SF Pro Rounded', Inter, system-ui, sans-serif"
        fontSize="82"
        fontWeight="900"
      >
        {BRAND_PREFIX}
      </text>
      <text
        x="148"
        y="106"
        className="wordmark-main"
        fontFamily="'Avenir Next Rounded', 'Avenir Next', 'Nunito Sans', ui-rounded, 'SF Pro Rounded', Inter, system-ui, sans-serif"
        fontSize="82"
        fontWeight="800"
      >
        {BRAND_SUFFIX}
      </text>
    </svg>
  );
};

export default HandyTextLogo;

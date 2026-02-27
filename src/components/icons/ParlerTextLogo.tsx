import React from "react";

const ParlerTextLogo = ({
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
      viewBox="0 0 400 80"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
    >
      <text
        x="200"
        y="58"
        textAnchor="middle"
        fontFamily="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif"
        fontSize="52"
        fontWeight="bold"
        className="logo-primary"
        style={{ letterSpacing: "-1px" }}
      >
        Parler
      </text>
    </svg>
  );
};

export default ParlerTextLogo;

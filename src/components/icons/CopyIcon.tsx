import React from "react";

interface CopyIconProps {
  width?: number;
  height?: number;
  color?: string;
  className?: string;
}

const CopyIcon: React.FC<CopyIconProps> = ({
  width = 24,
  height = 24,
  color = "#FAA2CA",
  className = "",
}) => {
  return (
    <svg
      width={width}
      height={height}
      viewBox="0 0 24 24"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      className={className}
    >
      <g fill={color}>
        <path
          d="M16 3H4v13h2v2H4a2 2 0 0 1-2-2V3a2 2 0 0 1 2-2h12a2 2 0 0 1 2 2v2h-2V3z"
          opacity=".4"
        />
        <path d="M8 7a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H10a2 2 0 0 1-2-2V7zm2 0v14h10V7H10z" />
      </g>
    </svg>
  );
};

export default CopyIcon;

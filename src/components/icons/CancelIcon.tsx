import React from "react";

interface CancelIconProps {
  width?: number;
  height?: number;
  color?: string;
  className?: string;
}

const CancelIcon: React.FC<CancelIconProps> = ({
  width = 32,
  height = 32,
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
        <path d="m14.293 8.29297c.3905-.39052 1.0235-.39052 1.414 0s.3905 1.02354 0 1.41406l-5.99998 5.99997c-.39053.3906-1.02354.3906-1.41407 0-.39052-.3905-.39052-1.0235 0-1.414z" />
        <path d="m8.29295 8.29297c.39053-.39052 1.02354-.39052 1.41407 0l5.99998 6.00003c.3905.3905.3905 1.0235 0 1.414-.3905.3906-1.0235.3906-1.414 0l-6.00005-5.99997c-.39052-.39052-.39052-1.02354 0-1.41406z" />
        <path
          d="m20 12c0-4.41828-3.5817-8-8-8-4.41828 0-8 3.58172-8 8 0 4.4183 3.58172 8 8 8 4.4183 0 8-3.5817 8-8zm2 0c0 5.5228-4.4772 10-10 10-5.52285 0-10-4.4772-10-10 0-5.52285 4.47715-10 10-10 5.5228 0 10 4.47715 10 10z"
          opacity=".4"
        />
      </g>
    </svg>
  );
};

export default CancelIcon;

import React from "react";

interface CheckIconProps {
  width?: number;
  height?: number;
  color?: string;
  className?: string;
}

const CheckIcon: React.FC<CheckIconProps> = ({
  width = 24,
  height = 24,
  color = "#4ade80",
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
        <path d="m9.99999 15.586-3.293-3.293c-.39053-.3905-1.02354-.3905-1.41407 0-.39052.3905-.39052 1.0236 0 1.4141l4 4c.39053.3905 1.02354.3905 1.41407 0l8.00001-8c.3905-.39053.3905-1.02354 0-1.41407-.3905-.39052-1.0236-.39052-1.4141 0z" />
        <path
          d="m20 12c0-4.41828-3.5817-8-8-8-4.41828 0-8 3.58172-8 8 0 4.4183 3.58172 8 8 8 4.4183 0 8-3.5817 8-8zm2 0c0 5.5228-4.4772 10-10 10-5.52285 0-10-4.4772-10-10 0-5.52285 4.47715-10 10-10 5.5228 0 10 4.47715 10 10z"
          opacity=".4"
        />
      </g>
    </svg>
  );
};

export default CheckIcon;

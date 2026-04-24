import React from "react";

interface TrashIconProps {
  width?: number;
  height?: number;
  className?: string;
}

const TrashIcon: React.FC<TrashIconProps> = ({
  width = 20,
  height = 20,
  className = "",
}) => (
  <svg
    width={width}
    height={height}
    viewBox="0 0 20 20"
    fill="none"
    xmlns="http://www.w3.org/2000/svg"
    className={className}
  >
    <g
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="1.5"
    >
      <path d="M3.5 5.5h13M8.5 5.5V4a.5.5 0 0 1 .5-.5h2a.5.5 0 0 1 .5.5v1.5M15.5 5.5l-.9 9.6a1 1 0 0 1-1 .9H6.4a1 1 0 0 1-1-.9L4.5 5.5" />
    </g>
  </svg>
);

export default TrashIcon;

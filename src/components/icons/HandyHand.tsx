interface HandyHandProps {
  width?: number | string;
  height?: number | string;
  className?: string;
}

const HandyHand = ({ width, height, className }: HandyHandProps) => (
  <svg
    width={width || 24}
    height={height || 24}
    viewBox="0 0 24 24"
    fill="none"
    className={className}
    xmlns="http://www.w3.org/2000/svg"
    aria-hidden="true"
  >
    <rect
      x="3"
      y="3"
      width="18"
      height="18"
      rx="5.5"
      fill="currentColor"
      opacity="0.16"
    />
    <path
      d="M7.2 7.8v8.4M16.8 7.8v8.4"
      stroke="currentColor"
      strokeWidth="2.1"
      strokeLinecap="round"
    />
    <path
      d="m9.15 8.65 5.7 6.7M14.85 8.65l-5.7 6.7"
      stroke="currentColor"
      strokeWidth="2.1"
      strokeLinecap="round"
    />
    <path
      d="M12 12.1v3.15"
      stroke="currentColor"
      strokeWidth="1.65"
      strokeLinecap="round"
    />
    <path
      d="M10.3 16.1h3.4"
      stroke="currentColor"
      strokeWidth="1.65"
      strokeLinecap="round"
    />
  </svg>
);

export default HandyHand;

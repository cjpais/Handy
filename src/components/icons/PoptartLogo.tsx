import React from "react";
import logo from "../../assets/poptart-logo.png";

const PoptartLogo = ({
  width,
  height,
  className,
}: {
  width?: number;
  height?: number;
  className?: string;
}) => {
  return (
    <img
      src={logo}
      width={width}
      height={height}
      className={className}
      alt="Poptart"
    />
  );
};

export default PoptartLogo;

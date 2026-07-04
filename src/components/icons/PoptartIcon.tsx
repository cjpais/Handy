import React from "react";
import icon from "../../assets/poptart-icon.png";

// ponytail: raster icon (source SVG is an embedded PNG), so no currentColor
// theme tinting; swap for a true vector if a monochrome version is drawn
const PoptartIcon = ({
  width,
  height,
  className,
}: {
  width?: number | string;
  height?: number | string;
  className?: string;
}) => (
  <img
    src={icon}
    width={width}
    height={height}
    className={className}
    alt=""
  />
);

export default PoptartIcon;

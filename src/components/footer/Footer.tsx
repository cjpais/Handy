import React from "react";

import ModelSelector from "../model-selector";

const Footer: React.FC = () => {
  return (
    <div className="flex justify-end items-center text-xs px-4 py-2 text-text/60">
      <ModelSelector />
    </div>
  );
};

export default Footer;

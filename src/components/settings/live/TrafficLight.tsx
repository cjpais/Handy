import React from "react";

export type TrafficLightStatus = "idle" | "green" | "yellow" | "red";

interface TrafficLightProps {
  status: TrafficLightStatus;
  size?: "sm" | "md" | "lg";
}

export const TrafficLight: React.FC<TrafficLightProps> = ({
  status,
  size = "md",
}) => {
  const sizeClasses = {
    sm: "w-4 h-4",
    md: "w-6 h-6",
    lg: "w-10 h-10",
  };

  const lightSize = sizeClasses[size];

  const getColor = (light: "red" | "yellow" | "green") => {
    if (status === "idle") {
      return "bg-mid-gray/30";
    }
    if (status === light) {
      switch (light) {
        case "green":
          return "bg-green-500 shadow-[0_0_12px_rgba(34,197,94,0.6)]";
        case "yellow":
          return "bg-yellow-500 shadow-[0_0_12px_rgba(234,179,8,0.6)]";
        case "red":
          return "bg-red-500 shadow-[0_0_12px_rgba(239,68,68,0.6)]";
      }
    }
    return "bg-mid-gray/30";
  };

  return (
    <div className="flex flex-col items-center gap-2 p-3 bg-background-dark/50 rounded-xl">
      <div
        className={`${lightSize} rounded-full transition-all duration-300 ${getColor("red")}`}
      />
      <div
        className={`${lightSize} rounded-full transition-all duration-300 ${getColor("yellow")}`}
      />
      <div
        className={`${lightSize} rounded-full transition-all duration-300 ${getColor("green")}`}
      />
    </div>
  );
};

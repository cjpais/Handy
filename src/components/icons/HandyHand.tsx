// Voxa replaces the Handy hand with its own mark; same path and props as
// upstream so imports keep working.
import { VoxaMark } from "@/voxa/brand/VoxaMark";

const HandyHand = ({
  width,
  height,
  className,
}: {
  width?: number | string;
  height?: number | string;
  className?: string;
}) => <VoxaMark width={width ?? 24} height={height} className={className} />;

export default HandyHand;

import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";

interface InputMethodProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export function InputMethod({
  descriptionMode = "tooltip",
  grouped = false,
}: InputMethodProps) {
  const { getSetting, updateSetting, isUpdating } = useSettings();

  const currentMethod = getSetting("input_method") ?? "type";

  const handleMethodChange = async (method: "paste" | "type") => {
    await updateSetting("input_method", method);
  };

  return (
    <SettingContainer
      title="Input Method"
      description="Choose how transcribed text is inserted into applications. 'Type' simulates keyboard typing (works better on Linux), 'Paste' uses clipboard (may be faster but less compatible)."
      descriptionMode={descriptionMode}
      grouped={grouped}
    >
      <div className="flex gap-2">
        <button
          type="button"
          onClick={() => handleMethodChange("type")}
          disabled={isUpdating("input_method")}
          className={`px-4 py-2 rounded-lg text-sm font-medium border transition-colors ${
            currentMethod === "type"
              ? "bg-blue-500 text-white border-blue-500"
              : "bg-gray-100 dark:bg-gray-700 text-gray-700 dark:text-gray-300 border-gray-300 dark:border-gray-600 hover:bg-gray-200 dark:hover:bg-gray-600"
          } disabled:opacity-50 disabled:cursor-not-allowed`}
        >
          Type
        </button>
        <button
          type="button"
          onClick={() => handleMethodChange("paste")}
          disabled={isUpdating("input_method")}
          className={`px-4 py-2 rounded-lg text-sm font-medium border transition-colors ${
            currentMethod === "paste"
              ? "bg-blue-500 text-white border-blue-500"
              : "bg-gray-100 dark:bg-gray-700 text-gray-700 dark:text-gray-300 border-gray-300 dark:border-gray-600 hover:bg-gray-200 dark:hover:bg-gray-600"
          } disabled:opacity-50 disabled:cursor-not-allowed`}
        >
          Paste
        </button>
      </div>
    </SettingContainer>
  );
}
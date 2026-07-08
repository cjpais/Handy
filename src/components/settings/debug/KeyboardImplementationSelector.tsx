import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { SettingContainer } from "../../ui/SettingContainer";
import { Dropdown, type DropdownOption } from "../../ui/Dropdown";
import { useSettings } from "../../../hooks/useSettings";
import { useOsType } from "../../../hooks/useOsType";
import { commands } from "@/bindings";
import { toast } from "sonner";
import { KeyboardPermissionsDialog } from "./KeyboardBackendPermissions";

const KEYBOARD_IMPLEMENTATION_OPTIONS: DropdownOption[] = [
  { value: "tauri", label: "Tauri Global Shortcut" },
  { value: "handy_keys", label: "Handy Keys" },
];

interface KeyboardImplementationSelectorProps {
  descriptionMode?: "tooltip" | "inline";
  grouped?: boolean;
}

export const KeyboardImplementationSelector: React.FC<
  KeyboardImplementationSelectorProps
> = ({ descriptionMode = "tooltip", grouped = false }) => {
  const { t } = useTranslation();
  const { getSetting, isUpdating, refreshSettings } = useSettings();
  const osType = useOsType();
  const currentImplementation =
    getSetting("keyboard_implementation") ?? "tauri";
  const [permissionsError, setPermissionsError] = useState<string | null>(null);
  const [permissionsDialogOpen, setPermissionsDialogOpen] = useState(false);

  const handleSelect = async (value: string) => {
    if (value === currentImplementation) return;

    try {
      const result = await commands.changeKeyboardImplementationSetting(value);

      if (result.status === "error") {
        console.error(
          "Failed to update keyboard implementation:",
          result.error,
        );
        // On Linux the handy-keys backend fails when input devices are not
        // accessible; walk the user through the fix instead of just toasting.
        if (osType === "linux" && value === "handy_keys") {
          setPermissionsError(String(result.error));
          setPermissionsDialogOpen(true);
        } else {
          toast.error(String(result.error));
        }
        // The backend rolled the setting back; reflect that in the dropdown
        await refreshSettings();
        return;
      }

      // If any bindings were reset due to incompatibility, notify the user
      if (result.data.reset_bindings.length > 0) {
        toast.warning(t("settings.debug.keyboardImplementation.bindingsReset"));
      }

      await refreshSettings();
    } catch (error) {
      console.error("Failed to update keyboard implementation:", error);
      toast.error(String(error));
    }
  };

  return (
    <>
      <SettingContainer
        title={t("settings.debug.keyboardImplementation.title")}
        description={t("settings.debug.keyboardImplementation.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
        layout="horizontal"
      >
        <Dropdown
          options={KEYBOARD_IMPLEMENTATION_OPTIONS}
          selectedValue={currentImplementation}
          onSelect={handleSelect}
          disabled={isUpdating("keyboard_implementation")}
        />
      </SettingContainer>
      <KeyboardPermissionsDialog
        open={permissionsDialogOpen}
        onOpenChange={setPermissionsDialogOpen}
        error={permissionsError}
      />
    </>
  );
};

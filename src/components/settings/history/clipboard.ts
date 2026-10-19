export type ClipboardWriter = Pick<Clipboard, "writeText">;

export const copyToClipboard = async (
  text: string,
  clipboard: ClipboardWriter = navigator.clipboard,
): Promise<boolean> => {
  try {
    await clipboard.writeText(text);
    return true;
  } catch (error) {
    console.error("Failed to copy to clipboard:", error);
    return false;
  }
};

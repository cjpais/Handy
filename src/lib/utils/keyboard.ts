/**
 * Keyboard utility functions for handling keyboard display
 *
 * Note: Key name conversion and normalization is now handled by the Rust backend.
 * This module only provides display formatting utilities.
 */

export type OSType = "macos" | "windows" | "linux" | "unknown";

/**
 * Format a key combination string for display
 *
 * The input is already in the correct format from the backend,
 * so this function just returns it as-is.
 * This function is kept for API compatibility and potential future formatting needs.
 */
export const formatKeyCombination = (
  combination: string,
  _osType: OSType
): string => {
  return combination;
};

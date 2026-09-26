import type { TFunction } from "i18next";
import type { PostProcessProfile } from "@/bindings";

export const DEFAULT_PROFILE_ID = "default";

/** Longest profile name the backend accepts (MAX_POST_PROCESS_PROFILE_NAME_LEN). */
export const MAX_PROFILE_NAME_LENGTH = 40;

/**
 * Label of a post-processing profile. The built-in profile stores no name
 * until renamed, so it is shown as "Default" in the current language.
 */
export function getProfileDisplayName(
  profile: Pick<PostProcessProfile, "name">,
  t: TFunction,
): string {
  const name = profile.name?.trim();
  return name || t("settings.postProcessing.profiles.defaultName");
}

import { type } from "@tauri-apps/plugin-os";

export interface ReleaseNote {
  version: string;
  markdown: string;
}

interface ReleaseNoteRecord {
  version: string;
  markdown: string;
}

interface FindReleaseNoteOptions {
  currentVersion: string;
  lastSeenVersion: string;
}

const releaseNoteModules = import.meta.glob<string>(
  "../../content/release-notes/*.md",
  {
    eager: true,
    import: "default",
    query: "?raw",
  },
);

type Platform = "linux" | "macos" | "windows";

interface ReleaseNoteParts {
  shared?: string;
  platforms: Partial<Record<Platform, string>>;
}

// `0.10.1.md` shows on every platform; `0.10.1.linux.md` only on Linux,
// after the shared note when there is one.
const releaseNotePartsByVersion = new Map<string, ReleaseNoteParts>();

const parseVersion = (version: string): [number, number, number] | null => {
  const normalized = version.trim().replace(/^v/i, "");
  const match = normalized.match(/^(\d+)\.(\d+)\.(\d+)/);
  if (!match) return null;

  return [Number(match[1]), Number(match[2]), Number(match[3])];
};

for (const [path, markdown] of Object.entries(releaseNoteModules)) {
  const match = path.match(
    /release-notes\/(\d+\.\d+\.\d+)(?:\.(linux|macos|windows))?\.md$/,
  );
  if (!match) continue;

  const version = match[1];
  const platform = match[2] as Platform | undefined;
  const parts = releaseNotePartsByVersion.get(version) ?? { platforms: {} };

  if (platform) {
    parts.platforms[platform] = markdown;
  } else {
    parts.shared = markdown;
  }

  releaseNotePartsByVersion.set(version, parts);
}

const currentPlatform = (): Platform | null => {
  try {
    const os = type();
    return os === "linux" || os === "macos" || os === "windows" ? os : null;
  } catch {
    return null;
  }
};

/** The notes this platform shows, by version. */
const releaseNotesForPlatform = (): ReleaseNoteRecord[] => {
  const platform = currentPlatform();
  const notes: ReleaseNoteRecord[] = [];

  for (const [version, parts] of releaseNotePartsByVersion) {
    const sections = [
      parts.shared,
      platform ? parts.platforms[platform] : undefined,
    ].filter((section): section is string => Boolean(section?.trim()));

    if (sections.length > 0) {
      notes.push({ version, markdown: sections.join("\n\n") });
    }
  }

  return notes;
};

const compareVersions = (a: string, b: string) => {
  const parsedA = parseVersion(a);
  const parsedB = parseVersion(b);

  if (!parsedA || !parsedB) return 0;

  for (let index = 0; index < parsedA.length; index += 1) {
    if (parsedA[index] !== parsedB[index]) {
      return parsedA[index] - parsedB[index];
    }
  }

  return 0;
};

export const findReleaseNoteToShow = ({
  currentVersion,
  lastSeenVersion,
}: FindReleaseNoteOptions): ReleaseNote | null => {
  if (!parseVersion(currentVersion)) return null;

  const hasValidLastSeenVersion = parseVersion(lastSeenVersion) !== null;
  const candidate = releaseNotesForPlatform()
    .filter((note) => compareVersions(note.version, currentVersion) <= 0)
    .filter(
      (note) =>
        !hasValidLastSeenVersion ||
        compareVersions(note.version, lastSeenVersion) > 0,
    )
    .sort((a, b) => compareVersions(b.version, a.version))[0];

  if (!candidate) return null;

  return candidate;
};

export const findLatestReleaseNote = (): ReleaseNote | null => {
  const candidate = releaseNotesForPlatform().sort((a, b) =>
    compareVersions(b.version, a.version),
  )[0];

  if (!candidate) return null;

  return candidate;
};

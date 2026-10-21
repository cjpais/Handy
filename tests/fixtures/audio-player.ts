import React from "react";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import {
  AudioPlayer,
  AudioPlayerGroup,
} from "../../src/components/ui/AudioPlayer";

interface PendingLoad {
  resolve: (src: string | null) => void;
  reject: (error: Error) => void;
}

export interface AudioPlayerFixture {
  created: string[];
  revoked: string[];
  requests: PendingLoad[];
  mount: (options?: {
    strict?: boolean;
    players?: number;
    initialBlob?: boolean;
  }) => void;
  unmount: () => void;
  resolve: (
    index: number,
    kind?: "blob" | "asset" | "null",
    unmountBeforeCommit?: boolean,
  ) => Promise<void>;
  reject: (index: number) => void;
}

declare global {
  interface Window {
    audioPlayerFixture: AudioPlayerFixture;
  }
}

// A small, valid thirty-second silent WAV. No microphone or user files are used.
function audioBlob(): Blob {
  const samples = 240_000;
  const bytes = new Uint8Array(44 + samples);
  const view = new DataView(bytes.buffer);
  for (const [offset, text] of [
    [0, "RIFF"],
    [8, "WAVEfmt "],
    [36, "data"],
  ] as const) {
    for (let i = 0; i < text.length; i++) {
      bytes[offset + i] = text.charCodeAt(i);
    }
  }
  view.setUint32(4, 36 + samples, true);
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, 8_000, true);
  view.setUint32(28, 8_000, true);
  view.setUint16(32, 1, true);
  view.setUint16(34, 8, true);
  view.setUint32(40, samples, true);
  bytes.fill(128, 44);
  return new Blob([bytes], { type: "audio/wav" });
}

const created: string[] = [];
const revoked: string[] = [];
const createUrl = URL.createObjectURL.bind(URL);
const revokeUrl = URL.revokeObjectURL.bind(URL);
URL.createObjectURL = (blob) => {
  const url = createUrl(blob);
  created.push(url);
  return url;
};
URL.revokeObjectURL = (url) => {
  revoked.push(url);
  revokeUrl(url);
};

let root: Root | null = null;
const requests: PendingLoad[] = [];
const fixture: AudioPlayerFixture = {
  created,
  revoked,
  requests,
  mount: ({ strict = false, players = 1, initialBlob = false } = {}) => {
    fixture.unmount();
    root = createRoot(document.getElementById("root")!);
    const content = React.createElement(
      AudioPlayerGroup,
      null,
      ...Array.from({ length: players }, (_, index) =>
        React.createElement(
          "section",
          { key: index, "data-testid": `player-${index}` },
          React.createElement(AudioPlayer, {
            src: initialBlob ? URL.createObjectURL(audioBlob()) : undefined,
            onLoadRequest: () =>
              new Promise<string | null>((resolve, reject) => {
                requests.push({ resolve, reject });
              }),
          }),
        ),
      ),
    );
    flushSync(() =>
      root!.render(
        strict ? React.createElement(React.StrictMode, null, content) : content,
      ),
    );
  },
  unmount: () => {
    if (root) flushSync(() => root!.unmount());
    root = null;
  },
  resolve: async (index, kind = "blob", unmountBeforeCommit = false) => {
    const src =
      kind === "blob"
        ? URL.createObjectURL(audioBlob())
        : kind === "asset"
          ? "asset://synthetic-recording.wav"
          : null;
    requests[index].resolve(src);
    if (unmountBeforeCommit) {
      // Let the player's await resume, then remove it before its state commits.
      await Promise.resolve();
      fixture.unmount();
    }
  },
  reject: (index) =>
    requests[index].reject(new Error("Synthetic read failure")),
};
window.audioPlayerFixture = fixture;

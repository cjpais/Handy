import React from "react";
import { useTranslation } from "react-i18next";
import { Slider } from "../ui/Slider";
import { useSettings } from "../../hooks/useSettings";

const NOTE_NAMES = ["C", "D", "E", "F", "G", "A", "B"];
// Semitones above C for each note of the C major scale.
const SCALE_STEPS = [0, 2, 4, 5, 7, 9, 11];

interface Note {
  name: string;
  hz: number;
}

// The white keys from E1 (41 Hz) to B5 (988 Hz).
const NOTES: Note[] = [];
for (let octave = 1; octave <= 6; octave++) {
  SCALE_STEPS.forEach((step, i) => {
    const midi = 12 * (octave + 1) + step;
    if (midi < 28 || midi > 83) return;
    NOTES.push({
      name: `${NOTE_NAMES[i]}${octave}`,
      hz: 440 * Math.pow(2, (midi - 69) / 12),
    });
  });
}

const DEFAULT_HZ = 261.63;

const nearestNoteIndex = (hz: number) => {
  let best = 0;
  NOTES.forEach((note, i) => {
    if (
      Math.abs(Math.log(note.hz / hz)) < Math.abs(Math.log(NOTES[best].hz / hz))
    ) {
      best = i;
    }
  });
  return best;
};

export const RecordingToneFrequency: React.FC<{ disabled?: boolean }> = ({
  disabled = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting } = useSettings();
  const noteIndex = nearestNoteIndex(
    getSetting("recording_tone_frequency") ?? DEFAULT_HZ,
  );

  return (
    <Slider
      value={noteIndex}
      onChange={(index: number) =>
        updateSetting("recording_tone_frequency", NOTES[index].hz)
      }
      min={0}
      max={NOTES.length - 1}
      step={1}
      label={t("settings.sound.recordingToneFrequency.title")}
      description={t("settings.sound.recordingToneFrequency.description")}
      descriptionMode="tooltip"
      grouped
      // Two lines so the label fits the same width as the other sliders.
      formatValue={(index) =>
        `${NOTES[index].name}\n${Math.round(NOTES[index].hz)}Hz`
      }
      disabled={disabled}
    />
  );
};

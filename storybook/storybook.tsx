import React, { useEffect, useMemo, useRef, useState } from "react";
import { Toaster } from "sonner";
import ProgressBar from "@/components/shared/ProgressBar";
import HandyTextLogo from "@/components/icons/HandyTextLogo";
import CancelIcon from "@/components/icons/CancelIcon";
import HandyHand from "@/components/icons/HandyHand";
import ResetIcon from "@/components/icons/ResetIcon";
import TranscriptionIcon from "@/components/icons/TranscriptionIcon";
import MicrophoneIcon from "@/components/icons/MicrophoneIcon";
import { AudioPlayer } from "@/components/ui/AudioPlayer";
import { Slider } from "@/components/ui/Slider";
import { SettingsGroup } from "@/components/ui/SettingsGroup";
import { SettingContainer } from "@/components/ui/SettingContainer";
import { TextDisplay } from "@/components/ui/TextDisplay";
import { Tooltip } from "@/components/ui/Tooltip";
import { Alert } from "@/components/ui/Alert";
import { ResetButton } from "@/components/ui/ResetButton";
import { Dropdown } from "@/components/ui/Dropdown";
import Badge from "@/components/ui/Badge";
import { ToggleSwitch } from "@/components/ui/ToggleSwitch";
import { IconButton } from "@/components/ui/IconButton";
import { PathDisplay } from "@/components/ui/PathDisplay";
import { Button } from "@/components/ui/Button";
import { Select } from "@/components/ui/Select";
import { Textarea } from "@/components/ui/Textarea";
import { Input } from "@/components/ui/Input";

// ─── Types ───────────────────────────────────────────────────────────
type ThemeMode = "light" | "dark";

interface StoryDef {
  id: string;
  name: string;
  group: string;
  render: () => React.ReactNode;
}

// ─── Helpers ─────────────────────────────────────────────────────────
const SILENT_AUDIO =
  "data:audio/wav;base64,UklGRiQAAABXQVZFZm10IBAAAAABAAEAIlYAAESsAAACABAAZGF0YQAAAAA=";

const Variant: React.FC<{ label: string; children: React.ReactNode }> = ({
  label,
  children,
}) => (
  <div className="sb-variant">
    <div className="sb-variant-label">{label}</div>
    <div className="sb-box">{children}</div>
  </div>
);

// ─── Story definitions ───────────────────────────────────────────────
function useStories() {
  const [toggleA, setToggleA] = useState(true);
  const [toggleB, setToggleB] = useState(false);
  const [slider, setSlider] = useState(0.65);
  const [ddVal, setDdVal] = useState("alpha");
  const [selVal, setSelVal] = useState<string | null>("alpha");
  const [crVal, setCrVal] = useState<string | null>("custom-llm");
  const tooltipRef = useRef<HTMLButtonElement>(null);

  const opts = useMemo(
    () => [
      { value: "alpha", label: "Alpha" },
      { value: "bravo", label: "Bravo" },
      { value: "charlie", label: "Charlie" },
    ],
    [],
  );

  const stories: StoryDef[] = useMemo(
    () => [
      // ── Design Tokens ──────────────────────────────────
      {
        id: "colors",
        name: "Colors",
        group: "Design Tokens",
        render: () => {
          const colorGroups: { label: string; tokens: { name: string; var: string }[] }[] = [
            {
              label: "Brand",
              tokens: [
                { name: "background-ui (Primary)", var: "--color-background-ui" },
                { name: "logo-primary", var: "--color-logo-primary" },
                { name: "logo-stroke", var: "--color-logo-stroke" },
              ],
            },
            {
              label: "Surface",
              tokens: [
                { name: "background", var: "--color-background" },
                { name: "text", var: "--color-text" },
                { name: "text-stroke", var: "--color-text-stroke" },
                { name: "mid-gray", var: "--color-mid-gray" },
              ],
            },
            {
              label: "Semantic",
              tokens: [
                { name: "error", var: "--color-error" },
                { name: "error-text", var: "--color-error-text" },
                { name: "error-bg", var: "--color-error-bg" },
                { name: "warning", var: "--color-warning" },
                { name: "warning-text", var: "--color-warning-text" },
                { name: "warning-bg", var: "--color-warning-bg" },
                { name: "info", var: "--color-info" },
                { name: "info-text", var: "--color-info-text" },
                { name: "info-bg", var: "--color-info-bg" },
                { name: "success", var: "--color-success" },
                { name: "success-text", var: "--color-success-text" },
                { name: "success-bg", var: "--color-success-bg" },
              ],
            },
            {
              label: "Primary Alpha",
              tokens: [
                { name: "primary-alpha-5", var: "--color-primary-alpha-5" },
                { name: "primary-alpha-10", var: "--color-primary-alpha-10" },
                { name: "primary-alpha-20", var: "--color-primary-alpha-20" },
                { name: "primary-alpha-30", var: "--color-primary-alpha-30" },
                { name: "primary-alpha-50", var: "--color-primary-alpha-50" },
                { name: "primary-alpha-80", var: "--color-primary-alpha-80" },
              ],
            },
            {
              label: "Gray Alpha",
              tokens: [
                { name: "gray-alpha-5", var: "--color-gray-alpha-5" },
                { name: "gray-alpha-10", var: "--color-gray-alpha-10" },
                { name: "gray-alpha-20", var: "--color-gray-alpha-20" },
                { name: "gray-alpha-40", var: "--color-gray-alpha-40" },
                { name: "gray-alpha-60", var: "--color-gray-alpha-60" },
                { name: "gray-alpha-80", var: "--color-gray-alpha-80" },
              ],
            },
            {
              label: "Text Alpha",
              tokens: [
                { name: "text-alpha-40", var: "--color-text-alpha-40" },
                { name: "text-alpha-50", var: "--color-text-alpha-50" },
                { name: "text-alpha-60", var: "--color-text-alpha-60" },
                { name: "text-alpha-70", var: "--color-text-alpha-70" },
                { name: "text-alpha-80", var: "--color-text-alpha-80" },
                { name: "text-alpha-90", var: "--color-text-alpha-90" },
              ],
            },
          ];
          return (
            <div style={{ display: "flex", flexDirection: "column", gap: 32 }}>
              {colorGroups.map((group) => (
                <div key={group.label}>
                  <div className="sb-variant-label" style={{ marginBottom: 12 }}>{group.label}</div>
                  <div style={{ display: "flex", flexWrap: "wrap", gap: 12 }}>
                    {group.tokens.map((token) => (
                      <div key={token.var} style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 6, width: 100 }}>
                        <div style={{
                          width: 64, height: 64, borderRadius: 10,
                          background: `var(${token.var})`,
                          border: "1px solid var(--sb-border)",
                          boxShadow: "0 1px 3px rgba(0,0,0,0.08)",
                        }} />
                        <span style={{ fontSize: 10, textAlign: "center", color: "var(--sb-dim)", lineHeight: 1.3, wordBreak: "break-all" }}>{token.name}</span>
                        <span style={{ fontSize: 9, fontFamily: "monospace", color: "var(--color-text)", opacity: 0.5, textAlign: "center" }}>{token.var}</span>
                      </div>
                    ))}
                  </div>
                </div>
              ))}
            </div>
          );
        },
      },
      {
        id: "typography",
        name: "Typography",
        group: "Design Tokens",
        render: () => {
          const typeScale = [
            { label: "Display", size: "28px", weight: "700", lineHeight: "1.2", sample: "Handy — Voice to Text" },
            { label: "Heading 1", size: "22px", weight: "600", lineHeight: "1.3", sample: "Settings & Preferences" },
            { label: "Heading 2", size: "18px", weight: "600", lineHeight: "1.3", sample: "General Settings" },
            { label: "Heading 3", size: "15px", weight: "600", lineHeight: "1.4", sample: "Recording Options" },
            { label: "Body", size: "14px", weight: "400", lineHeight: "1.5", sample: "Select your preferred microphone and configure how recordings are handled." },
            { label: "Body Medium", size: "14px", weight: "500", lineHeight: "1.5", sample: "Push to talk is enabled" },
            { label: "Small", size: "13px", weight: "400", lineHeight: "1.5", sample: "Choose an audio input device from the list below." },
            { label: "Small Medium", size: "13px", weight: "500", lineHeight: "1.5", sample: "Model downloaded successfully" },
            { label: "Caption", size: "12px", weight: "400", lineHeight: "1.4", sample: "Last updated 3 minutes ago" },
            { label: "Label", size: "11px", weight: "600", lineHeight: "1.4", sample: "AUDIO INPUT", style: "uppercase" as const, letterSpacing: "0.05em" },
            { label: "Mono", size: "13px", weight: "400", lineHeight: "1.5", sample: "sk-ant-api03-xxxxxxxxxxxxxx", fontFamily: "monospace" },
          ];
          return (
            <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
              <div style={{ display: "grid", gridTemplateColumns: "120px 80px 60px auto", gap: "8px 16px", marginBottom: 8, fontSize: 10, fontWeight: 600, textTransform: "uppercase", letterSpacing: "0.05em", color: "var(--sb-dim)", padding: "0 0 8px", borderBottom: "1px solid var(--sb-border)" }}>
                <span>Style</span>
                <span>Size / Weight</span>
                <span>Line Height</span>
                <span>Sample</span>
              </div>
              {typeScale.map((row) => (
                <div key={row.label} style={{ display: "grid", gridTemplateColumns: "120px 80px 60px auto", gap: "8px 16px", alignItems: "baseline", padding: "10px 0", borderBottom: "1px solid var(--sb-border)" }}>
                  <span style={{ fontSize: 11, fontWeight: 600, color: "var(--sb-dim)" }}>{row.label}</span>
                  <span style={{ fontSize: 10, fontFamily: "monospace", color: "var(--sb-dim)" }}>{row.size} / {row.weight}</span>
                  <span style={{ fontSize: 10, fontFamily: "monospace", color: "var(--sb-dim)" }}>{row.lineHeight}</span>
                  <span style={{
                    fontSize: row.size,
                    fontWeight: row.weight as React.CSSProperties["fontWeight"],
                    lineHeight: row.lineHeight,
                    fontFamily: row.fontFamily ?? "inherit",
                    textTransform: row.style ?? "none",
                    letterSpacing: row.letterSpacing ?? "normal",
                    color: "var(--color-text)",
                  }}>{row.sample}</span>
                </div>
              ))}
            </div>
          );
        },
      },

      // ── Icons ──────────────────────────────────────────
      {
        id: "icons",
        name: "All Icons",
        group: "Icons",
        render: () => (
          <Variant label="Icon set">
            <div className="sb-box-row" style={{ gap: 32 }}>
              {[
                { label: "HandyTextLogo", node: <HandyTextLogo width={100} /> },
                { label: "HandyHand", node: <HandyHand width={32} height={32} /> },
                { label: "TranscriptionIcon", node: <TranscriptionIcon width={28} height={28} /> },
                { label: "MicrophoneIcon", node: <MicrophoneIcon width={28} height={28} /> },
                { label: "ResetIcon", node: <ResetIcon /> },
                { label: "CancelIcon", node: <CancelIcon /> },
              ].map(({ label, node }) => (
                <div key={label} className="flex flex-col items-center gap-1">
                  {node}
                  <span className="text-xs" style={{ color: "var(--sb-dim)" }}>{label}</span>
                </div>
              ))}
            </div>
          </Variant>
        ),
      },

      // ── UI Components ──────────────────────────────────
      {
        id: "button",
        name: "Button",
        group: "UI",
        render: () => (
          <>
            <Variant label="Variants">
              <div className="sb-box-row">
                <Button>Primary</Button>
                <Button variant="primary-soft">Primary Soft</Button>
                <Button variant="secondary">Secondary</Button>
                <Button variant="danger">Danger</Button>
                <Button variant="danger-ghost">Danger Ghost</Button>
                <Button variant="ghost">Ghost</Button>
              </div>
            </Variant>
            <Variant label="Sizes">
              <div className="sb-box-row">
                <Button size="sm">Small</Button>
                <Button size="md">Medium</Button>
                <Button size="lg">Large</Button>
              </div>
            </Variant>
            <Variant label="Disabled">
              <div className="sb-box-row">
                <Button disabled>Primary Disabled</Button>
                <Button variant="secondary" disabled>Secondary Disabled</Button>
                <Button variant="danger" disabled>Danger Disabled</Button>
              </div>
            </Variant>
          </>
        ),
      },
      {
        id: "icon-button",
        name: "IconButton",
        group: "UI",
        render: () => (
          <>
            <Variant label="Variants">
              <div className="sb-box-row">
                <IconButton aria-label="Primary" variant="primary" icon={<ResetIcon />} />
                <IconButton aria-label="Secondary" variant="secondary" icon={<ResetIcon />} />
                <IconButton aria-label="Danger" variant="danger" icon={<CancelIcon />} />
                <IconButton aria-label="Ghost" variant="ghost" icon={<MicrophoneIcon />} />
              </div>
            </Variant>
            <Variant label="Sizes">
              <div className="sb-box-row">
                <IconButton aria-label="Small" size="sm" icon={<ResetIcon />} />
                <IconButton aria-label="Medium" size="md" icon={<ResetIcon />} />
                <IconButton aria-label="Large" size="lg" icon={<ResetIcon />} />
              </div>
            </Variant>
            <Variant label="Disabled">
              <div className="sb-box-row">
                <IconButton aria-label="Disabled" disabled icon={<ResetIcon />} />
              </div>
            </Variant>
          </>
        ),
      },
      {
        id: "input",
        name: "Input",
        group: "UI",
        render: () => (
          <>
            <Variant label="Default">
              <div className="sb-box-col">
                <Input placeholder="Default input" />
              </div>
            </Variant>
            <Variant label="Compact">
              <div className="sb-box-col">
                <Input variant="compact" placeholder="Compact input" />
              </div>
            </Variant>
            <Variant label="Disabled">
              <div className="sb-box-col">
                <Input disabled placeholder="Disabled input" />
              </div>
            </Variant>
          </>
        ),
      },
      {
        id: "textarea",
        name: "Textarea",
        group: "UI",
        render: () => (
          <>
            <Variant label="Default">
              <Textarea placeholder="Write a note..." rows={3} />
            </Variant>
            <Variant label="Compact">
              <Textarea variant="compact" placeholder="Compact textarea" rows={2} />
            </Variant>
          </>
        ),
      },
      {
        id: "select",
        name: "Select",
        group: "UI",
        render: () => (
          <>
            <Variant label="Standard">
              <Select
                value={selVal}
                options={opts}
                onChange={(v) => setSelVal(v)}
                placeholder="Pick an option"
              />
            </Variant>
            <Variant label="Creatable">
              <Select
                value={crVal}
                options={opts}
                onChange={(v) => setCrVal(v)}
                onCreateOption={(v) => setCrVal(v)}
                isCreatable
                placeholder="Type to create"
              />
            </Variant>
          </>
        ),
      },
      {
        id: "dropdown",
        name: "Dropdown",
        group: "UI",
        render: () => (
          <>
            <Variant label="Default">
              <Dropdown options={opts} selectedValue={ddVal} onSelect={setDdVal} />
            </Variant>
            <Variant label="Disabled">
              <Dropdown options={opts} selectedValue={ddVal} onSelect={setDdVal} disabled />
            </Variant>
          </>
        ),
      },
      {
        id: "toggle-switch",
        name: "ToggleSwitch",
        group: "UI",
        render: () => (
          <>
            <Variant label="Tooltip description (default)">
              <ToggleSwitch checked={toggleA} onChange={setToggleA} label="Push to Talk" description="Hold shortcut key to record" />
            </Variant>
            <Variant label="Inline description">
              <ToggleSwitch checked={toggleB} onChange={setToggleB} label="Always-on Microphone" description="Keeps mic active at all times" descriptionMode="inline" />
            </Variant>
            <Variant label="Grouped (inside SettingsGroup)">
              <SettingsGroup title="Audio">
                <ToggleSwitch checked={toggleA} onChange={setToggleA} label="Push to Talk" description="Hold shortcut key to record" grouped />
                <ToggleSwitch checked={toggleB} onChange={setToggleB} label="Mute while recording" description="Silences other audio" grouped />
              </SettingsGroup>
            </Variant>
          </>
        ),
      },
      {
        id: "slider",
        name: "Slider",
        group: "UI",
        render: () => (
          <>
            <Variant label="Tooltip description (default)">
              <Slider
                value={slider}
                onChange={setSlider}
                min={0}
                max={1}
                step={0.05}
                label="Volume"
                description="Adjust output level"
                formatValue={(v) => `${Math.round(v * 100)}%`}
              />
            </Variant>
            <Variant label="Inline description">
              <Slider
                value={slider}
                onChange={setSlider}
                min={0}
                max={1}
                step={0.05}
                label="Playback speed"
                description="Controls how fast audio plays back"
                descriptionMode="inline"
                formatValue={(v) => `${Math.round(v * 100)}%`}
              />
            </Variant>
            <Variant label="Grouped (inside SettingsGroup)">
              <SettingsGroup title="Playback">
                <Slider
                  value={slider}
                  onChange={setSlider}
                  min={0}
                  max={1}
                  step={0.05}
                  label="Volume"
                  description="Adjust output level"
                  formatValue={(v) => `${Math.round(v * 100)}%`}
                  grouped
                />
              </SettingsGroup>
            </Variant>
          </>
        ),
      },
      {
        id: "badge",
        name: "Badge",
        group: "UI",
        render: () => (
          <Variant label="Variants">
            <div className="sb-box-row">
              <Badge variant="primary">primary</Badge>
              <Badge variant="secondary">secondary</Badge>
              <Badge variant="success">success</Badge>
            </div>
          </Variant>
        ),
      },
      {
        id: "alert",
        name: "Alert",
        group: "UI",
        render: () => (
          <Variant label="All variants">
            <div className="sb-box-col">
              <Alert variant="error">Error message</Alert>
              <Alert variant="warning">Warning message</Alert>
              <Alert variant="info">Info message</Alert>
              <Alert variant="success">Success message</Alert>
            </div>
          </Variant>
        ),
      },
      {
        id: "tooltip",
        name: "Tooltip",
        group: "UI",
        render: () => (
          <Variant label="Default">
            <button
              ref={tooltipRef}
              className="px-3 py-2 text-xs font-semibold rounded-md border border-mid-gray/40 bg-mid-gray/10"
            >
              Hover target
            </button>
            <Tooltip targetRef={tooltipRef} position="top">
              <p className="text-sm text-center">Tooltip content</p>
            </Tooltip>
          </Variant>
        ),
      },
      {
        id: "text-display",
        name: "TextDisplay",
        group: "UI",
        render: () => (
          <>
            <Variant label="Copyable + Monospace">
              <TextDisplay label="API Key" description="Your stored key" value="sk-demo-key-xxxxxxxxxxxx" copyable monospace />
            </Variant>
            <Variant label="Plain">
              <TextDisplay label="Status" description="Current connection status" value="Connected" />
            </Variant>
            <Variant label="Grouped (inside SettingsGroup)">
              <SettingsGroup title="API">
                <TextDisplay label="Base URL" description="Endpoint for requests" value="https://api.example.com/v1" copyable grouped />
              </SettingsGroup>
            </Variant>
          </>
        ),
      },
      {
        id: "path-display",
        name: "PathDisplay",
        group: "UI",
        render: () => (
          <Variant label="Default">
            <PathDisplay path="/Users/edward/Library/Application Support/Handy" onOpen={() => {}} />
          </Variant>
        ),
      },
      {
        id: "reset-button",
        name: "ResetButton",
        group: "UI",
        render: () => (
          <Variant label="Default">
            <div className="sb-box-row">
              <ResetButton onClick={() => {}} />
              <ResetButton onClick={() => {}} disabled />
            </div>
          </Variant>
        ),
      },
      {
        id: "audio-player",
        name: "AudioPlayer",
        group: "UI",
        render: () => (
          <Variant label="Default">
            <AudioPlayer src={SILENT_AUDIO} />
          </Variant>
        ),
      },
      {
        id: "setting-container",
        name: "SettingContainer",
        group: "UI",
        render: () => (
          <>
            <Variant label="Horizontal + Tooltip">
              <SettingContainer title="Setting Name" description="A tooltip description" descriptionMode="tooltip">
                <Button size="sm">Action</Button>
              </SettingContainer>
            </Variant>
            <Variant label="Horizontal + Inline">
              <SettingContainer title="Setting Name" description="An inline description" descriptionMode="inline">
                <Button size="sm" variant="secondary">Action</Button>
              </SettingContainer>
            </Variant>
            <Variant label="Stacked">
              <SettingContainer title="Setting Name" description="A stacked layout" layout="stacked">
                <Input placeholder="Enter value" />
              </SettingContainer>
            </Variant>
          </>
        ),
      },
      {
        id: "settings-group",
        name: "SettingsGroup",
        group: "UI",
        render: () => (
          <Variant label="Grouped settings">
            <SettingsGroup title="Example Group" description="Multiple related settings">
              <SettingContainer title="Setting A" description="First option" descriptionMode="tooltip" grouped>
                <Button size="sm">Action</Button>
              </SettingContainer>
              <SettingContainer title="Setting B" description="Second option" descriptionMode="inline" grouped>
                <Button size="sm" variant="secondary">Secondary</Button>
              </SettingContainer>
            </SettingsGroup>
          </Variant>
        ),
      },

      // ── Shared ──────────────────────────────────────────
      {
        id: "progress-bar",
        name: "ProgressBar",
        group: "Shared",
        render: () => (
          <>
            <Variant label="Single — medium (default) + showSpeed">
              <ProgressBar progress={[{ id: "one", percentage: 45, speed: 8.2 }]} showSpeed />
            </Variant>
            <Variant label="Single — small">
              <ProgressBar size="small" progress={[{ id: "s", percentage: 70 }]} />
            </Variant>
            <Variant label="Single — large + showLabel">
              <ProgressBar size="large" progress={[{ id: "l", percentage: 30, label: "whisper-base.en" }]} showLabel />
            </Variant>
            <Variant label="Multiple">
              <ProgressBar
                progress={[
                  { id: "a", percentage: 25 },
                  { id: "b", percentage: 60 },
                  { id: "c", percentage: 80 },
                ]}
              />
            </Variant>
          </>
        ),
      },
    ],
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [toggleA, toggleB, slider, ddVal, selVal, crVal, opts],
  );

  return stories;
}

// ─── Sidebar nav arrow SVG ───────────────────────────────────────────
const ChevronDown = () => (
  <svg viewBox="0 0 10 10" fill="currentColor">
    <path d="M2 3.5L5 6.5L8 3.5" stroke="currentColor" strokeWidth="1.5" fill="none" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
);

// ─── Main App ────────────────────────────────────────────────────────
export const StorybookApp: React.FC = () => {
  const stories = useStories();

  const [theme, setTheme] = useState<ThemeMode>(() => {
    if (typeof window === "undefined") return "light";
    return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  });

  const [activeId, setActiveId] = useState(stories[0]?.id ?? "");
  const [openGroups, setOpenGroups] = useState<Record<string, boolean>>({});

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  const groups = useMemo(() => {
    const map = new Map<string, StoryDef[]>();
    for (const s of stories) {
      if (!map.has(s.group)) map.set(s.group, []);
      map.get(s.group)!.push(s);
    }
    return Array.from(map.entries());
  }, [stories]);

  useEffect(() => {
    const defaults: Record<string, boolean> = {};
    for (const [g] of groups) defaults[g] = true;
    setOpenGroups(defaults);
  }, [groups]);

  const toggleGroup = (g: string) =>
    setOpenGroups((prev) => ({ ...prev, [g]: !prev[g] }));

  const active = stories.find((s) => s.id === activeId);
  const activeGroup = active?.group ?? "";

  return (
    <div className="sb-root">
      <Toaster theme={theme} richColors />

      {/* Sidebar */}
      <nav className="sb-sidebar">
        <div className="sb-sidebar-head">
          <HandyTextLogo width={70} />
          <span>Storybook</span>
        </div>

        <div className="sb-sidebar-scroll">
          {groups.map(([group, items]) => (
            <div key={group}>
              <button
                className="sb-group-btn"
                data-open={String(openGroups[group] !== false)}
                onClick={() => toggleGroup(group)}
              >
                <ChevronDown />
                {group}
              </button>
              <div className="sb-group-list" data-open={String(openGroups[group] !== false)}>
                {items.map((s) => (
                  <button
                    key={s.id}
                    className="sb-nav-item"
                    data-active={String(s.id === activeId)}
                    onClick={() => setActiveId(s.id)}
                  >
                    {s.name}
                  </button>
                ))}
              </div>
            </div>
          ))}
        </div>

        <div className="sb-sidebar-foot">
          <div className="sb-theme-row">
            <button className="sb-theme-btn" data-active={String(theme === "light")} onClick={() => setTheme("light")}>
              Light
            </button>
            <button className="sb-theme-btn" data-active={String(theme === "dark")} onClick={() => setTheme("dark")}>
              Dark
            </button>
          </div>
        </div>
      </nav>

      {/* Main area */}
      <div className="sb-main">
        <div className="sb-toolbar">
          <span>{activeGroup}</span>
          <span className="sb-toolbar-sep">/</span>
          <span className="sb-toolbar-cur">{active?.name ?? ""}</span>
        </div>

        <div className="sb-canvas" key={activeId}>
          <div className="sb-canvas-inner">
            <div className="sb-story-name">{active?.name}</div>
            <div className="sb-story-hint">{activeGroup}</div>
            {active?.render()}
          </div>
        </div>
      </div>
    </div>
  );
};

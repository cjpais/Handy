import { expect, test } from "@playwright/test";
import { getMockState, setMockState, setupMocks } from "./helpers";

test.describe("Meeting Assistant", () => {
  test.beforeEach(async ({ page }) => {
    await setupMocks(page, false);
    await page.goto("/");
  });

  const promptPayload = {
    mode: "suggestion",
    prompt: {
      provider: "Google Meet",
      title: "Design Review",
      source: "LocalDetection",
      start_time: "2026-06-22T10:00:00Z",
      join_url: "https://meet.google.com/abc-defg-hij",
    },
    recording_started_at: null,
  };

  test("prompt Record transitions to meeting recording command flow", async ({
    page,
  }) => {
    await page.goto("/src/meeting_prompt/index.html");
    await page.evaluate((payload) => {
      (window as any).__EMIT_EVENT__("meeting-overlay-show", {
        ...payload,
        prompt: {
          ...payload.prompt,
          start_time: "2026-06-22T10:00:05Z",
        },
      });
    }, promptPayload);

    await expect(page.getByText("Meeting detected")).toBeVisible();
    await page.getByRole("button", { name: "Record" }).click();
    await expect(page.getByText("Recording")).toBeVisible();

    const state = await getMockState(page);
    expect(state.promptEvents).toContainEqual({ action: "start" });
  });

  test("dismiss and timer suppress repeated prompts for the same meeting", async ({
    page,
  }) => {
    await page.goto("/src/meeting_prompt/index.html");
    await page.evaluate((payload) => {
      (window as any).__EMIT_EVENT__("meeting-overlay-show", payload);
    }, promptPayload);

    await expect(page.getByText("Meeting detected")).toBeVisible();
    await page.waitForFunction(
      () =>
        (window as any).__MOCK_STATE__.promptEvents.filter(
          (event: any) => event.action === "dismiss",
        ).length === 1,
      undefined,
      { timeout: 9500 },
    );

    let state = await getMockState(page);
    expect(
      state.promptEvents.filter((event) => event.action === "dismiss"),
    ).toHaveLength(1);

    await page.evaluate((payload) => {
      (window as any).__EMIT_EVENT__("meeting-overlay-show", payload);
    }, promptPayload);

    state = await getMockState(page);
    expect(
      state.promptEvents.filter((event) => event.action === "dismiss"),
    ).toHaveLength(1);
  });

  test("stop overlay command records a stop event rather than canceling", async ({
    page,
  }) => {
    await page.evaluate(async () => {
      const mod = await import("/src/bindings.ts");
      await mod.commands.startMeetingRecordingFromPrompt();
      await mod.commands.stopMeetingRecordingFromOverlay();
    });

    const state = await getMockState(page);
    expect(state.promptEvents).toEqual([
      { action: "start" },
      { action: "stop" },
    ]);
  });

  test("instant meeting placeholder appears as a meeting entry and later updates", async ({
    page,
  }) => {
    await setMockState(page, {
      historyEntries: [
        {
          id: 7,
          file_name: "meeting_pending.wav",
          timestamp: Math.floor(Date.now() / 1000),
          saved: false,
          title: "Jun 22, 2026, 10:00 AM",
          transcription_text: "",
          post_processed_text: null,
          post_process_prompt: "default_meeting_notes_with_actions",
          post_process_requested: true,
        },
      ],
    });

    await page.reload();
    await page.getByText("Meetings").click();
    await expect(page.getByText("Processing")).toBeVisible();

    await setMockState(page, {
      historyEntries: [
        {
          id: 7,
          file_name: "meeting_pending.wav",
          timestamp: Math.floor(Date.now() / 1000),
          saved: false,
          title: "Jun 22, 2026, 10:00 AM",
          transcription_text: "Transcript is ready.",
          post_processed_text: "Summary is ready.",
          post_process_prompt: "default_meeting_notes_with_actions",
          post_process_requested: true,
        },
      ],
    });

    await page.reload();
    await page.getByText("Meetings").click();
    await expect(page.getByText("Summary is ready.")).toBeVisible();
  });

  test("calendar prompt can be dismissed and calendar auth stays independent", async ({
    page,
  }) => {
    await page.getByText("Meetings").click();

    await setMockState(page, { calendarConnected: true });
    await page.reload();
    await page.getByText("Meetings").click();

    await expect(page.locator("text=Google Calendar Prompts")).toBeVisible();
    await expect(
      page.locator("text=Connected for meeting reminders"),
    ).toBeVisible();
    await expect(page.locator(".send-via-google-btn")).not.toBeVisible();

    await page.evaluate(async () => {
      const mod = await import("/src/bindings.ts");
      await mod.commands.setMeetingCalendarPromptsEnabled(true);
    });

    const stateAfterToggle = await getMockState(page);
    expect(stateAfterToggle.calendarPromptsEnabled).toBe(true);
    expect(stateAfterToggle.gmailTasksConnected).toBe(false);

    await page.evaluate(async () => {
      const mod = await import("/src/bindings.ts");
      await mod.commands.dismissMeetingPrompt({
        provider: "Google Meet",
        title: "Design Review",
        source: "GoogleCalendar",
        start_time: new Date().toISOString(),
        join_url: "https://meet.google.com/abc-defg-hij",
      });
    });
    const finalState = await getMockState(page);
    expect(
      finalState.promptEvents.some((event) => event.action === "dismiss"),
    ).toBe(true);
  });

  test("meeting prompt settings persist and calendar prompts stay gated by calendar auth", async ({
    page,
  }) => {
    await page.getByText("Meetings").click();

    const calendarToggle = page.locator(
      "xpath=//h3[normalize-space()='Calendar Prompts']/ancestor::div[contains(@class,'justify-between')][1]//input[@type='checkbox']",
    );

    await expect(calendarToggle).toBeDisabled();

    await page.evaluate(async () => {
      const mod = await import("/src/bindings.ts");
      await mod.commands.changeMeetingDetectionEnabledSetting(true);
      await mod.commands.changeMeetingPromptLeadMinutesSetting(5);
    });

    await page.reload();
    await page.getByText("Meetings").click();

    const state = await getMockState(page);
    expect(state.meetingDetectionEnabled).toBe(true);
    expect(state.meetingPromptLeadMinutes).toBe(5);

    await expect(page.getByText("Prompt Lead Time")).toBeVisible();
  });

  test("oauth-unavailable state disables Google meeting integrations", async ({
    page,
  }) => {
    await setMockState(page, { oauthClientConfigured: false });
    await page.reload();
    await page.getByText("Meetings").click();

    await expect(
      page
        .getByText(
          "Google Calendar is unavailable until a desktop OAuth client ID is configured for this build.",
        )
        .first(),
    ).toBeVisible();

    await expect(page.locator(".google-connect-btn")).toBeDisabled();

    const calendarToggle = page.locator(
      "xpath=//h3[normalize-space()='Calendar Prompts']/ancestor::div[contains(@class,'justify-between')][1]//input[@type='checkbox']",
    );
    await expect(calendarToggle).toBeDisabled();
  });
});

import { expect, test } from "@playwright/test";
import { getMockState, setMockState, setupMocks } from "./helpers";

test.describe("Meeting Assistant", () => {
  test.beforeEach(async ({ page }) => {
    await setupMocks(page, false);
    await page.goto("/");
  });

  test("local detected meeting prompt starts recording", async ({ page }) => {
    await page.evaluate(async () => {
      const mod = await import("/src/bindings.ts");
      await mod.commands.startMeetingRecordingFromPrompt();
    });

    const state = await getMockState(page);
    expect(state.promptEvents).toContainEqual({ action: "start" });
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

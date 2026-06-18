import { expect, test } from "@playwright/test";
import { getMockState, setMockState, setupMocks } from "./helpers";

test.describe("Meeting Assistant", () => {
  test("local detected meeting prompt starts recording", async ({ page }) => {
    await setupMocks(page, false);
    await page.goto("/");
    await page.evaluate(async () => {
      const mod = await import("/src/bindings.ts");
      await mod.commands.startMeetingRecordingFromPrompt();
    });

    const state = await getMockState(page);
    expect(state.promptEvents).toContainEqual({ action: "start" });
  });

  test("calendar prompt can be dismissed and calendar toggle is independent", async ({
    page,
  }) => {
    await setupMocks(page, false);
    await page.goto("/");
    await page.click("text=Meetings");

    await setMockState(page, { calendarConnected: true });
    await page.reload();
    await page.click("text=Meetings");

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
});

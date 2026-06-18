import { test, expect } from "@playwright/test";
import { setupMocks, getMockState, setMockState } from "./helpers";

test.describe("Google Integration E2E Tests", () => {
  test.beforeEach(async ({ page }) => {
    // Start with Google Services disconnected by default
    await setupMocks(page, false);
    await page.goto("/");
    // Click on the Meetings tab
    await page.click("text=Meetings");
  });

  test("Tier 1: Connect, Disconnect Google Services, and Send Follow-Up", async ({
    page,
  }) => {
    // 1. Assert initial disconnected state
    await expect(
      page
        .locator(
          "text=Connect to send meeting follow-ups via Gmail and create tasks",
        )
        .first(),
    ).toBeVisible();
    await expect(page.locator(".google-connect-btn")).toBeVisible();
    await expect(page.locator(".send-via-google-btn")).not.toBeVisible();

    // 2. Connect Google Services
    await page.click(".google-connect-btn");
    await expect(
      page.locator("text=Connected to Gmail & Google Tasks").first(),
    ).toBeVisible();
    await expect(page.locator(".google-disconnect-btn")).toBeVisible();

    // 3. Send Follow-Up
    // Verify the "Send via Google" button is now visible and click it
    const sendBtn = page.locator(".send-via-google-btn");
    await expect(sendBtn).toBeVisible();
    await sendBtn.click();

    // Dialog should open
    await expect(page.locator(".follow-up-dialog")).toBeVisible();

    // Fill recipients
    await page.fill(".recipients-input", "alex@example.com, john@example.com");
    await page.click(".send-btn");

    // Dialog should close after success
    await expect(page.locator(".follow-up-dialog")).not.toBeVisible();

    // Verify mock state matches expected data sent
    const mockState = await getMockState(page);
    expect(mockState.gmailTasksConnected).toBe(true);
    expect(mockState.lastFollowUp).not.toBeNull();
    expect(mockState.lastFollowUp?.recipients).toEqual([
      "alex@example.com",
      "john@example.com",
    ]);
    expect(mockState.lastFollowUp?.summary).toBe(
      "Project kickoff meeting to discuss architecture.",
    );
    expect(mockState.lastFollowUp?.actionItems).toEqual([
      "Build tests first",
      "Verify and document",
    ]);

    // 4. Disconnect Google Services
    await page.click(".google-disconnect-btn");
    await expect(
      page
        .locator(
          "text=Connect to send meeting follow-ups via Gmail and create tasks",
        )
        .first(),
    ).toBeVisible();
    await expect(page.locator(".google-connect-btn")).toBeVisible();
    await expect(page.locator(".send-via-google-btn")).not.toBeVisible();
  });

  test("Tier 2: Validation boundaries and failure handling", async ({
    page,
  }) => {
    // Connect Google Services
    await page.click(".google-connect-btn");
    await expect(page.locator(".send-via-google-btn")).toBeVisible();
    await page.click(".send-via-google-btn");

    // 1. Validation boundary: Empty email input
    await page.click(".send-btn");
    await expect(page.locator(".error-message")).toContainText(
      "Recipient email is required.",
    );

    // 2. Validation boundary: Invalid email input
    await page.fill(".recipients-input", "invalid-email");
    await page.click(".send-btn");
    await expect(page.locator(".error-message")).toContainText(
      "Invalid email address: invalid-email",
    );

    // Close the dialog
    await page.click(".cancel-btn");
    await expect(page.locator(".follow-up-dialog")).not.toBeVisible();

    // 3. OAuth Connection Failure
    await page.click(".google-disconnect-btn");
    await setMockState(page, { oauthSuccess: false });
    await page.click(".google-connect-btn");
    // State should remain disconnected
    await expect(
      page
        .locator(
          "text=Connect to send meeting follow-ups via Gmail and create tasks",
        )
        .first(),
    ).toBeVisible();

    // 4. Send Follow-up Failure
    await setMockState(page, { oauthSuccess: true, sendSuccess: false });
    await page.click(".google-connect-btn");
    await page.click(".send-via-google-btn");
    await page.fill(".recipients-input", "alex@example.com");
    await page.click(".send-btn");
    // Dialog should stay open on error
    await expect(page.locator(".follow-up-dialog")).toBeVisible();
  });

  test("Tier 3: Button visibility based on status and post-processing match", async ({
    page,
  }) => {
    // 1. Disconnected -> Send via Google button should be hidden
    await expect(page.locator(".send-via-google-btn")).not.toBeVisible();

    // 2. Connected -> Send via Google button should be visible
    await page.click(".google-connect-btn");
    await expect(page.locator(".send-via-google-btn")).toBeVisible();

    // 3. Post-processing integration match
    await page.click(".send-via-google-btn");
    await page.fill(".recipients-input", "alex@example.com");
    await page.click(".send-btn");

    const mockState = await getMockState(page);
    // The summary and action items sent should exactly match the post-processed JSON content of our mock meeting entry
    expect(mockState.lastFollowUp?.summary).toBe(
      "Project kickoff meeting to discuss architecture.",
    );
    expect(mockState.lastFollowUp?.actionItems).toEqual([
      "Build tests first",
      "Verify and document",
    ]);
  });

  test("Tier 4: Workload scalability, multiple recipients, and loading states", async ({
    page,
  }) => {
    await page.click(".google-connect-btn");
    await page.click(".send-via-google-btn");

    // Fill multiple emails with various delimiters (commas and spaces)
    await page.fill(
      ".recipients-input",
      "alex@example.com, john@example.com   kate@example.com",
    );

    // Trigger sending follow-up
    await page.click(".send-btn");

    // Dialog should close, and all 3 recipients should be parsed correctly
    await expect(page.locator(".follow-up-dialog")).not.toBeVisible();

    const mockState = await getMockState(page);
    expect(mockState.lastFollowUp?.recipients).toEqual([
      "alex@example.com",
      "john@example.com",
      "kate@example.com",
    ]);
  });
});

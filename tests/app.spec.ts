import { test, expect } from "@playwright/test";

test.describe("Handy App", () => {
  test("dev server responds", async ({ page }) => {
    // Just verify the dev server is running and responds
    const response = await page.goto("/");
    expect(response?.status()).toBe(200);
  });

  test("page has html structure", async ({ page }) => {
    await page.goto("/");

    // Verify basic HTML structure exists
    const html = await page.content();
    expect(html).toContain("<html");
    expect(html).toContain("<body");
  });

  test("external transcription model helper exposes ElevenLabs profile", async ({
    page,
  }) => {
    await page.goto("/");

    const data = await page.evaluate(async () => {
      const mod = await import("/src/lib/utils/externalTranscriptionModel.ts");

      return {
        profile: mod.getElevenLabsModelProfile(),
        activeName: mod.getActiveTranscriptionModelDisplayName("elevenlabs"),
        hasKey: mod.hasTranscriptionProviderApiKey("elevenlabs", {
          elevenlabs: "  sk-test  ",
        }),
        missingKey: mod.hasTranscriptionProviderApiKey("elevenlabs", {
          elevenlabs: "   ",
        }),
      };
    });

    expect(data.profile.fullLabel).toBe("Scribe v2 by ElevenLabs");
    expect(data.profile.description).toContain("Highest-accuracy");
    expect(data.activeName).toBe("Scribe v2 by ElevenLabs");
    expect(data.hasKey).toBe(true);
    expect(data.missingKey).toBe(false);
  });

  test("english translations include cloud model copy", async ({ page }) => {
    await page.goto("/");

    const strings = await page.evaluate(async () => {
      const mod = await import("/src/i18n/locales/en/translation.json");
      return {
        yourModels: mod.default.settings.models.yourModels,
        cloudModels: mod.default.settings.models.cloudModels,
        addApiKey: mod.default.settings.models.external.addApiKey,
        transcriptionFailedTitle: mod.default.errors.transcriptionFailedTitle,
      };
    });

    expect(strings.yourModels).toBe("Installed Models");
    expect(strings.cloudModels).toBe("Cloud Models");
    expect(strings.addApiKey).toBe("Add API key");
    expect(strings.transcriptionFailedTitle).toBe("Transcription failed");
  });
});

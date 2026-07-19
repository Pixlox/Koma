import { afterEach, describe, expect, it } from "vitest";

import { applyLanguage, listAvailableLanguages } from "../i18n";
import { errorMessage, localizeMessage } from "./backend";

afterEach(async () => {
  await applyLanguage("en");
});

describe("localized backend messages", () => {
  it("maps stable command error codes without exposing English diagnostics", async () => {
    await applyLanguage("fr");

    expect(
      errorMessage({
        code: "missing_source",
        message: "the source is unavailable: /tmp/book.cbz",
        recoverable: true,
      }),
    ).toBe("Le fichier source est introuvable.");
  });

  it("uses a translated safe fallback for unknown backend errors", async () => {
    await applyLanguage("fr");

    expect(errorMessage(new Error("database exploded in English"))).toBe(
      "L’opération n’a pas pu être effectuée.",
    );
  });

  it("localizes dynamic backend text through its surface-specific fallback", async () => {
    await applyLanguage("fr");

    expect(
      localizeMessage(
        "unknown English folder error",
        "The last folder scan could not be completed.",
      ),
    ).toBe("La dernière analyse du dossier n’a pas pu être effectuée.");
  });

  it("keeps detailed diagnostics in English", async () => {
    await applyLanguage("en");

    expect(
      errorMessage({
        code: "missing_source",
        message: "the source is unavailable: /tmp/book.cbz",
        recoverable: true,
      }),
    ).toBe("the source is unavailable: /tmp/book.cbz");
  });

  it("loads localized failures for every bundled non-English language", async () => {
    const languages = listAvailableLanguages().filter(
      ({ locale }) => locale !== "en",
    );
    expect(languages).toHaveLength(10);

    for (const language of languages) {
      await applyLanguage(language.locale);
      expect(
        errorMessage({
          code: "missing_source",
          message: "the source is unavailable: /tmp/book.cbz",
          recoverable: true,
        }),
        language.locale,
      ).not.toBe("the source is unavailable: /tmp/book.cbz");
      expect(
        errorMessage(new Error("database exploded in English")),
        language.locale,
      ).not.toBe("The operation could not be completed.");
    }
  });
});

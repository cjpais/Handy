import { test, expect } from "@playwright/test";
import {
  DictionaryParseError,
  WORDS_MAX_LENGTH,
  buildDictionaryJson,
  buildWordsTxt,
  detectDictionaryFormat,
  filterReplacements,
  filterWords,
  mergeReplacements,
  mergeWords,
  parseDictionaryFile,
  parseDictionaryJson,
  parseReplacementsCsv,
  parseWordsTxt,
  serializeReplacementsCsv,
  validateReplacement,
  validateWord,
} from "../src/components/settings/dictionary/dictionaryFile";

test.describe("dictionary validation", () => {
  test("word validation: empty, too long, duplicate (case-insensitive)", () => {
    expect(validateWord("   ", ["OpenRouter"])).toBe("empty");
    expect(validateWord("x".repeat(WORDS_MAX_LENGTH + 1), [])).toBe("tooLong");
    expect(validateWord("openrouter", ["OpenRouter"])).toBe("duplicate");
    expect(validateWord("OpenRouter", ["OpenRouter"])).toBe("duplicate");
    expect(validateWord("ThinkPad", ["OpenRouter"])).toBeNull();
  });

  test("word normalization mirrors manual-add behavior", () => {
    expect(validateWord('  <Open "Router">  ', [])).toBeNull();
    // normalizeWord strips [<>"'] but does not merge the space those characters
    // leave behind, so '<Open "Router">' normalizes to "Open Router" — not
    // "OpenRouter". Upstream parity means this is therefore NOT a duplicate.
    expect(validateWord('  <Open "Router">  ', ["OpenRouter"])).toBeNull();
    expect(validateWord('  <Open "Router">  ', ["Open Router"])).toBe(
      "duplicate",
    );
  });

  test("replacement validation: blank sides and duplicate source", () => {
    const rules = [{ from: "open router", to: "OpenRouter" }];
    expect(validateReplacement("", "x", rules)).toBe("emptyFrom");
    expect(validateReplacement("   ", "x", rules)).toBe("emptyFrom");
    expect(validateReplacement("x", "", rules)).toBe("emptyTo");
    expect(validateReplacement("Open Router", "y", rules)).toBe("duplicate");
    expect(validateReplacement("OPEN ROUTER", "y", rules)).toBe("duplicate");
    expect(validateReplacement("open router", "y", rules, 0)).toBeNull();
  });
});

test.describe("dictionary search", () => {
  const words = ["OpenRouter", "ThinkPad", "Tauri", "Darkroom Edits"];
  const rules = [
    { from: "open router", to: "OpenRouter" },
    { from: "think pad", to: "ThinkPad" },
  ];

  test("word search is case-insensitive and never reorders", () => {
    expect(filterWords(words, "open")).toEqual(["OpenRouter"]);
    expect(filterWords(words, "THINK")).toEqual(["ThinkPad"]);
    expect(filterWords(words, "")).toEqual(words);
    expect(filterWords(words, "  ")).toEqual(words);
    expect(filterWords(words, "zzz")).toEqual([]);
    expect(words).toEqual([
      "OpenRouter",
      "ThinkPad",
      "Tauri",
      "Darkroom Edits",
    ]);
  });

  test("replacement search matches both sides", () => {
    expect(filterReplacements(rules, "router")).toHaveLength(1);
    expect(filterReplacements(rules, "think")).toHaveLength(1);
    expect(filterReplacements(rules, "ThinkPad")).toHaveLength(1);
    expect(filterReplacements(rules, "nope")).toHaveLength(0);
    expect(filterReplacements(rules, "")).toEqual(rules);
  });
});

test.describe("TXT parsing", () => {
  test("LF and CRLF, blank lines, surrounding whitespace, duplicates", () => {
    const content = "OpenRouter\n\n  ThinkPad  \r\nTauri\r\nOpenRouter\n";
    expect(parseWordsTxt(content)).toEqual([
      "OpenRouter",
      "ThinkPad",
      "Tauri",
      "OpenRouter",
    ]);
  });

  test("blank file yields no words", () => {
    expect(parseWordsTxt("")).toEqual([]);
    expect(parseWordsTxt("\n\n  \r\n")).toEqual([]);
  });
});

test.describe("CSV parsing", () => {
  test("normal rows with and without header", () => {
    expect(
      parseReplacementsCsv("open router,OpenRouter\nthink pad,ThinkPad\n")
        .rules,
    ).toEqual([
      { from: "open router", to: "OpenRouter" },
      { from: "think pad", to: "ThinkPad" },
    ]);
    expect(
      parseReplacementsCsv("from,to\nopen router,OpenRouter\n").rules,
    ).toEqual([{ from: "open router", to: "OpenRouter" }]);
  });

  test("quoted commas, quotes, and unicode", () => {
    const content =
      'from,to\n"hello, world","dark room edits"\n"say ""hi""","שלום"\n';
    const { rules, malformedRows } = parseReplacementsCsv(content);
    expect(malformedRows).toBe(0);
    expect(rules).toEqual([
      { from: "hello, world", to: "dark room edits" },
      { from: 'say "hi"', to: "שלום" },
    ]);
  });

  test("malformed rows are skipped and counted, not applied", () => {
    const content = "a,b\nonly-one-column\ntoo,many,columns\n,\n\nok,fine\n";
    const { rules, malformedRows } = parseReplacementsCsv(content);
    expect(rules).toEqual([
      { from: "a", to: "b" },
      { from: "ok", to: "fine" },
    ]);
    expect(malformedRows).toBe(3);
  });

  test("unterminated quote rejects the whole file", () => {
    expect(() => parseReplacementsCsv('a,"oops\n')).toThrow(
      DictionaryParseError,
    );
    try {
      parseReplacementsCsv('a,"oops\n');
    } catch (error) {
      expect((error as DictionaryParseError).code).toBe("malformedCsv");
    }
  });

  test("csv roundtrip: serialize then parse", () => {
    const rules = [
      { from: "open router", to: "OpenRouter" },
      { from: "with,comma", to: 'and "quotes"' },
      { from: "שלום", to: "עולם" },
    ];
    expect(parseReplacementsCsv(serializeReplacementsCsv(rules)).rules).toEqual(
      rules,
    );
  });
});

test.describe("JSON parsing", () => {
  const good = {
    schema: "handy-dictionary",
    version: 1,
    customWords: ["OpenRouter", "ThinkPad"],
    replacements: [{ from: "open router", to: "OpenRouter" }],
  };

  test("full export/import roundtrip", () => {
    const json = buildDictionaryJson(
      ["OpenRouter", "Tauri"],
      [{ from: "n eight n", to: "n8n" }],
    );
    const parsed = parseDictionaryJson(json);
    expect(parsed.words).toEqual(["OpenRouter", "Tauri"]);
    expect(parsed.replacements).toEqual([{ from: "n eight n", to: "n8n" }]);
  });

  test("empty dictionary parses", () => {
    const parsed = parseDictionaryJson(
      JSON.stringify({
        schema: "handy-dictionary",
        version: 1,
        customWords: [],
        replacements: [],
      }),
    );
    expect(parsed).toEqual({ words: [], replacements: [] });
  });

  test("malformed JSON is rejected", () => {
    expect(() => parseDictionaryJson("{not json")).toThrow(
      DictionaryParseError,
    );
  });

  test("unsupported version is rejected with detail", () => {
    try {
      parseDictionaryJson(JSON.stringify({ ...good, version: 99 }));
      throw new Error("expected parse failure");
    } catch (error) {
      expect(error).toBeInstanceOf(DictionaryParseError);
      expect((error as DictionaryParseError).code).toBe("unsupportedVersion");
      expect((error as DictionaryParseError).detail).toBe("99");
    }
  });

  test("wrong schema or shapes are rejected", () => {
    expect(() =>
      parseDictionaryJson(JSON.stringify({ ...good, schema: "other" })),
    ).toThrow(DictionaryParseError);
    expect(() =>
      parseDictionaryJson(JSON.stringify({ ...good, customWords: [1, 2] })),
    ).toThrow(DictionaryParseError);
    expect(() =>
      parseDictionaryJson(
        JSON.stringify({ ...good, replacements: [{ from: "a" }] }),
      ),
    ).toThrow(DictionaryParseError);
    expect(() => parseDictionaryJson("[1,2,3]")).toThrow(DictionaryParseError);
  });

  test("unicode words and rules survive", () => {
    const json = buildDictionaryJson(
      ["שלום", "CJ's Studio"],
      [{ from: "dark room edits", to: "Darkroom Edits" }],
    );
    const parsed = parseDictionaryJson(json);
    expect(parsed.words).toEqual(["שלום", "CJ's Studio"]);
  });
});

test.describe("import merging", () => {
  test("words merge: dedupe case-insensitively, count added/skipped", () => {
    const result = mergeWords(
      ["OpenRouter", "Tauri"],
      [
        "openrouter",
        "Tauri",
        "ThinkPad",
        "",
        "   ",
        "x".repeat(WORDS_MAX_LENGTH + 1),
        "Darkroom Edits",
      ],
    );
    expect(result.words).toEqual([
      "OpenRouter",
      "Tauri",
      "ThinkPad",
      "Darkroom Edits",
    ]);
    expect(result.added).toBe(2);
    // 2 case-insensitive duplicates + 2 blanks + 1 over-length entry.
    expect(result.skipped).toBe(5);
    expect(result.duplicates).toBe(2);
    expect(result.invalid).toBe(3);
  });

  test("replacements merge: conflicts keep existing, blanks skipped", () => {
    const result = mergeReplacements(
      [{ from: "open router", to: "OpenRouter" }],
      [
        { from: "OPEN ROUTER", to: "OtherValue" },
        { from: "think pad", to: "ThinkPad" },
        { from: "  ", to: "x" },
        { from: "x", to: "" },
        { from: "Think Pad", to: "dupe of import" },
      ],
    );
    expect(result.rules).toEqual([
      { from: "open router", to: "OpenRouter" },
      { from: "think pad", to: "ThinkPad" },
    ]);
    expect(result.added).toBe(1);
    expect(result.skipped).toBe(2);
    expect(result.conflicts).toBe(2);
  });

  test("importing into an empty dictionary adds everything", () => {
    expect(mergeWords([], ["OpenRouter"]).added).toBe(1);
    expect(mergeReplacements([], [{ from: "a", to: "b" }]).added).toBe(1);
  });

  test("import paths never re-strip punctuation (round trip stays lossless)", () => {
    // Every import path (JSON, TXT, merge) normalizes for import only: trim +
    // collapse whitespace. "CJ's Studio" must survive an export → import
    // round trip byte-for-byte, so the manual-add normalizeWord (which strips
    // [<>"']) must never touch stored data.
    const json = buildDictionaryJson(["CJ's Studio", 'say "hi"'], []);
    expect(parseDictionaryJson(json).words).toEqual([
      "CJ's Studio",
      'say "hi"',
    ]);
    expect(parseWordsTxt(buildWordsTxt(["CJ's Studio"]))).toEqual([
      "CJ's Studio",
    ]);
    const merged = mergeWords([], ["CJ's Studio", 'say "hi"']);
    expect(merged.words).toEqual(["CJ's Studio", 'say "hi"']);
    expect(merged.added).toBe(2);
    expect(merged.skipped).toBe(0);
    expect(mergeWords(["CJ's Studio"], ["  CJ's Studio  "])).toMatchObject({
      words: ["CJ's Studio"],
      added: 0,
      duplicates: 1,
    });
  });
});

test.describe("file format detection", () => {
  test("by extension, then by content", () => {
    expect(detectDictionaryFormat("d.json", "")).toBe("json");
    expect(detectDictionaryFormat("d.CSV", "")).toBe("csv");
    expect(detectDictionaryFormat("d.txt", "")).toBe("txt");
    expect(detectDictionaryFormat("d", '{"schema":')).toBe("json");
    expect(detectDictionaryFormat("d", "from,to\na,b\n")).toBe("csv");
    expect(detectDictionaryFormat("d", "OpenRouter\n")).toBe("txt");
  });

  test("parseDictionaryFile routes and reports malformed rows", () => {
    expect(
      parseDictionaryFile("words.txt", "OpenRouter\n\nTauri\n").words,
    ).toEqual(["OpenRouter", "Tauri"]);
    const csv = parseDictionaryFile("r.csv", "a,b\nbroken\n");
    expect(csv.replacements).toEqual([{ from: "a", to: "b" }]);
    expect(csv.malformedRows).toBe(1);
  });

  test("TXT export helper writes one entry per line", () => {
    expect(buildWordsTxt(["OpenRouter", "Tauri"])).toBe("OpenRouter\nTauri\n");
    expect(buildWordsTxt([])).toBe("");
  });
});

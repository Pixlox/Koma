# Contributing a Koma language

Languages ship with Koma. Readers select them in **Settings → Appearance →
Language**; there is no language-pack import step.

To contribute a language:

1. Fork `Pixlox/Koma`.
2. Copy [`examples/fr.locale.json`](examples/fr.locale.json) to
   `src/locales/<locale>.locale.json`.
3. Set `locale` to a valid BCP 47 tag such as `fr`, `pt-BR`, or `zh-Hant`.
4. Set `name` in English and `nativeName` in the language itself.
5. Translate every interface key in `translations`.
6. Run `npm run i18n:check` and `npm run typecheck`.
7. Open a pull request with screenshots of Settings, the library, the importer,
   and the reader.

Vite discovers every `src/locales/*.locale.json` file at build time. A valid
new file is bundled automatically and appears in the language dropdown without
editing TypeScript or registering the locale anywhere else.

## Translation rules

- Keep every key in English. Only change its value.
- Preserve placeholders exactly, including their braces:

  ```json
  "Page {{current}} of {{total}}": "Page {{current}} sur {{total}}"
  ```

- Keep keyboard shortcuts, file formats, product names, and `Koma` unchanged.
- Use native punctuation and concise labels that fit the existing controls.
- Check plural forms and long strings in the actual interface.
- English, Japanese, Spanish, French, Indonesian, Korean, Russian, Thai,
  Vietnamese, Simplified Chinese, and Traditional Chinese are already bundled;
  their locale codes are reserved.

The file format is defined by
[`language.schema.json`](language.schema.json). The i18n check rejects malformed
locale files, duplicate locale codes, missing interface keys, unknown keys, and
changed placeholders.

When English interface text changes, the same check reports which contributed
languages need updating before the build can pass.

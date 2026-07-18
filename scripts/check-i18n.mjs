import fs from "node:fs";
import path from "node:path";

function sourceFiles(directory) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const target = path.join(directory, entry.name);
    if (entry.isDirectory()) return sourceFiles(target);
    return /\.(?:ts|tsx)$/.test(entry.name) ? [target] : [];
  });
}

function catalogueKeys(source, start, end) {
  const segment = source.slice(source.indexOf(start), source.indexOf(end));
  const keys = [
    ...segment.matchAll(/^    ("(?:[^"\\]|\\.)*"):/gm),
  ].map((match) => JSON.parse(match[1]));
  const duplicates = keys.filter((key, index) => keys.indexOf(key) !== index);
  if (duplicates.length > 0) {
    throw new Error(`duplicate translations: ${[...new Set(duplicates)].join(", ")}`);
  }
  return new Set(keys);
}

const i18nSource = fs.readFileSync("src/i18n.ts", "utf8");
const catalogues = {
  ja: catalogueKeys(i18nSource, "  ja: {", "  es: {"),
  es: catalogueKeys(i18nSource, "  es: {", "\n} as const"),
};
const staticKeys = new Set();

for (const file of sourceFiles("src")) {
  if (file === "src/i18n.ts") continue;
  const source = fs.readFileSync(file, "utf8");
  for (const match of source.matchAll(/\btr\(\s*("(?:[^"\\]|\\.)*")/g)) {
    staticKeys.add(JSON.parse(match[1]));
  }
}

const reference = catalogues.ja;
const errors = [];
const localeCodes = new Set(["en", "ja", "es"]);

function placeholders(value) {
  return [
    ...value.matchAll(/{{\s*([^{}]+?)\s*}}/g),
  ].map((match) => match[1]).sort();
}

function checkPlaceholders(language, entries) {
  for (const [key, value] of entries) {
    const expected = placeholders(key);
    const actual = placeholders(value);
    if (JSON.stringify(expected) !== JSON.stringify(actual)) {
      errors.push(`${language} changed placeholders in: ${key}`);
    }
  }
}

for (const [language, keys] of Object.entries(catalogues)) {
  const missing = [...staticKeys].filter((key) => !keys.has(key));
  const catalogueDrift = [
    ...[...reference].filter((key) => !keys.has(key)),
    ...[...keys].filter((key) => !reference.has(key)),
  ];
  if (missing.length > 0) {
    errors.push(`${language} is missing: ${missing.sort().join(", ")}`);
  }
  if (catalogueDrift.length > 0) {
    errors.push(`${language} catalogue differs: ${catalogueDrift.sort().join(", ")}`);
  }
  const segment = i18nSource.slice(
    i18nSource.indexOf(`  ${language}: {`),
    language === "ja"
      ? i18nSource.indexOf("  es: {")
      : i18nSource.indexOf("\n} as const"),
  );
  const entries = [
    ...segment.matchAll(/^    ("(?:[^"\\]|\\.)*"):\s*("(?:[^"\\]|\\.)*"),?$/gm),
  ].map((match) => [JSON.parse(match[1]), JSON.parse(match[2])]);
  checkPlaceholders(language, entries);
}

const localeDirectory = "src/locales";
const localeFiles = fs
  .readdirSync(localeDirectory, { withFileTypes: true })
  .filter((entry) => entry.isFile() && entry.name.endsWith(".locale.json"))
  .map((entry) => path.join(localeDirectory, entry.name));

for (const file of localeFiles) {
  let locale;
  try {
    locale = JSON.parse(fs.readFileSync(file, "utf8"));
  } catch (error) {
    errors.push(`${file} is not valid JSON: ${error.message}`);
    continue;
  }
  if (
    locale === null ||
    Array.isArray(locale) ||
    typeof locale !== "object" ||
    locale.schemaVersion !== 1 ||
    typeof locale.locale !== "string" ||
    typeof locale.name !== "string" ||
    typeof locale.nativeName !== "string" ||
    locale.translations === null ||
    Array.isArray(locale.translations) ||
    typeof locale.translations !== "object"
  ) {
    errors.push(`${file} does not match the locale format`);
    continue;
  }
  const allowedFields = new Set([
    "$schema",
    "schemaVersion",
    "locale",
    "name",
    "nativeName",
    "translations",
  ]);
  const unknownFields = Object.keys(locale).filter(
    (field) => !allowedFields.has(field),
  );
  if (unknownFields.length > 0) {
    errors.push(`${file} has unknown fields: ${unknownFields.join(", ")}`);
  }
  if (
    locale.name.trim().length === 0 ||
    locale.name.length > 80 ||
    locale.nativeName.trim().length === 0 ||
    locale.nativeName.length > 80
  ) {
    errors.push(`${file} has an invalid language name`);
  }
  let canonical;
  try {
    canonical = Intl.getCanonicalLocales(locale.locale)[0];
  } catch {
    errors.push(`${file} has an invalid BCP 47 locale`);
    continue;
  }
  if (canonical !== locale.locale) {
    errors.push(`${file} locale must use canonical form: ${canonical}`);
  }
  const code = locale.locale.toLowerCase();
  if (localeCodes.has(code)) {
    errors.push(`${file} duplicates or reserves locale: ${locale.locale}`);
  }
  localeCodes.add(code);
  const expectedFile = `${locale.locale}.locale.json`;
  if (path.basename(file) !== expectedFile) {
    errors.push(`${file} must be named ${expectedFile}`);
  }
  const translations = Object.entries(locale.translations);
  const keys = new Set(translations.map(([key]) => key));
  const missing = [...reference].filter((key) => !keys.has(key));
  const unknown = [...keys].filter((key) => !reference.has(key));
  if (missing.length > 0) {
    errors.push(`${locale.locale} is missing: ${missing.sort().join(", ")}`);
  }
  if (unknown.length > 0) {
    errors.push(`${locale.locale} has unknown keys: ${unknown.sort().join(", ")}`);
  }
  if (translations.some(([, value]) => typeof value !== "string")) {
    errors.push(`${locale.locale} contains a non-string translation`);
  } else {
    checkPlaceholders(locale.locale, translations);
  }
}

if (errors.length > 0) {
  errors.forEach((error) => console.error(`i18n-check: ${error}`));
  process.exit(1);
}

console.log(
  `i18n-check: OK (${staticKeys.size} static keys, ${reference.size} translated keys, ${localeCodes.size} bundled locales)`,
);

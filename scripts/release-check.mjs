import fs from "node:fs";

const packageJson = JSON.parse(fs.readFileSync("package.json", "utf8"));
const packageLock = JSON.parse(fs.readFileSync("package-lock.json", "utf8"));
const tauri = JSON.parse(fs.readFileSync("src-tauri/tauri.conf.json", "utf8"));
const releaseWorkflow = fs.readFileSync(
  ".github/workflows/release.yml",
  "utf8",
);
const cargo = fs.readFileSync("Cargo.toml", "utf8");
const cargoVersion = cargo.match(
  /\[workspace\.package\][\s\S]*?\nversion\s*=\s*"([^"]+)"/,
)?.[1];
const versions = [
  packageJson.version,
  packageLock.version,
  packageLock.packages?.[""]?.version,
  tauri.version,
  cargoVersion,
];
const errors = [];
if (new Set(versions).size !== 1) {
  errors.push(`version mismatch: ${versions.join(", ")}`);
}
const tag = process.env.GITHUB_REF_NAME ?? process.env.RELEASE_TAG ?? "";
if (tag.startsWith("v") && tag.slice(1) !== packageJson.version) {
  errors.push(`tag ${tag} does not match version ${packageJson.version}`);
}
const updater = tauri.plugins?.updater;
if (
  updater?.endpoints?.[0] !==
  "https://github.com/Pixlox/Koma/releases/latest/download/latest.json"
) {
  errors.push("unexpected updater endpoint");
}
if (typeof updater?.pubkey !== "string" || updater.pubkey.length < 100) {
  errors.push("updater public key is missing");
}
if (tauri.bundle?.createUpdaterArtifacts !== true) {
  errors.push("updater artifacts are disabled");
}
const requiredReleaseSettings = [
  ["Tauri release action", "uses: tauri-apps/tauri-action@action-v0.6.2"],
  ["tag-based release creation", "tagName: ${{ github.ref_name }}"],
  ["draft release creation", "releaseDraft: true"],
  ["generated release notes", "generateReleaseNotes: true"],
  ["updater JSON upload", "uploadUpdaterJson: true"],
  ["updater signature upload", "uploadUpdaterSignatures: true"],
  ["workflow artifact upload disabled", "uploadWorkflowArtifacts: false"],
  ["Windows installers", "bundles: nsis,msi"],
  ["macOS installers", "bundles: app,dmg"],
  ["Linux installers", "bundles: appimage,deb,rpm"],
];
for (const [description, setting] of requiredReleaseSettings) {
  if (!releaseWorkflow.includes(setting)) {
    errors.push(`${description} is missing from the release workflow`);
  }
}
const requiredIcons = [
  "icons/32x32.png",
  "icons/128x128.png",
  "icons/128x128@2x.png",
  "icons/icon.icns",
  "icons/icon.ico",
];
const configuredIcons = new Set(tauri.bundle?.icon ?? []);
for (const icon of requiredIcons) {
  if (!configuredIcons.has(icon)) {
    errors.push(`distribution icon is not configured: ${icon}`);
  }
  const path = `src-tauri/${icon}`;
  if (!fs.existsSync(path) || fs.statSync(path).size === 0) {
    errors.push(`distribution icon is missing: ${path}`);
  }
}
if (errors.length > 0) {
  errors.forEach((error) => console.error(`release-check: ${error}`));
  process.exit(1);
}
console.log(`release-check: OK (Koma ${packageJson.version})`);

import fs from "node:fs";
import path from "node:path";

const root = process.cwd();
const version = (process.argv[2] ?? "").replace(/^v/, "");
if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version)) {
  console.error("usage: npm run release:version -- 1.2.3");
  process.exit(1);
}

function json(relativePath) {
  return JSON.parse(fs.readFileSync(path.join(root, relativePath), "utf8"));
}

function writeJson(relativePath, value) {
  fs.writeFileSync(
    path.join(root, relativePath),
    `${JSON.stringify(value, null, 2)}\n`,
  );
}

for (const relativePath of ["package.json", "package-lock.json"]) {
  const value = json(relativePath);
  value.version = version;
  if (relativePath === "package-lock.json" && value.packages?.[""]) {
    value.packages[""].version = version;
  }
  writeJson(relativePath, value);
}

const tauri = json("src-tauri/tauri.conf.json");
tauri.version = version;
writeJson("src-tauri/tauri.conf.json", tauri);

const cargoPath = path.join(root, "Cargo.toml");
const cargo = fs.readFileSync(cargoPath, "utf8");
const nextCargo = cargo.replace(
  /(\[workspace\.package\][\s\S]*?\nversion\s*=\s*)"[^"]+"/,
  `$1"${version}"`,
);
if (nextCargo === cargo) {
  throw new Error("workspace package version was not found");
}
fs.writeFileSync(cargoPath, nextCargo);

const appleInfoPath = path.join(
  root,
  "src-tauri/gen/apple/koma_iOS/Info.plist",
);
if (fs.existsSync(appleInfoPath)) {
  const appleInfo = fs.readFileSync(appleInfoPath, "utf8");
  const nextAppleInfo = appleInfo
    .replace(
      /(<key>CFBundleShortVersionString<\/key>\s*<string>)[^<]+/,
      `$1${version}`,
    )
    .replace(
      /(<key>CFBundleVersion<\/key>\s*<string>)[^<]+/,
      `$1${version}`,
    );
  fs.writeFileSync(appleInfoPath, nextAppleInfo);
}

const appleProjectPath = path.join(root, "src-tauri/gen/apple/project.yml");
if (fs.existsSync(appleProjectPath)) {
  const appleProject = fs.readFileSync(appleProjectPath, "utf8");
  const nextAppleProject = appleProject
    .replace(
      /(CFBundleShortVersionString:\s*)[^\s]+/,
      `$1${version}`,
    )
    .replace(/(CFBundleVersion:\s*)"[^"]+"/, `$1"${version}"`);
  fs.writeFileSync(appleProjectPath, nextAppleProject);
}

console.log(`Koma version set to ${version}`);

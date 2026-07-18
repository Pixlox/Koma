import { getVersion } from "@tauri-apps/api/app";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";

import { backend } from "./backend";

export interface UpdateInfo {
  version: string;
  currentVersion: string;
  body: string | null;
  date: string | null;
}

export interface AvailableUpdate {
  info: UpdateInfo;
  downloadAndInstall(
    onEvent: (event: DownloadEvent) => void,
  ): Promise<void>;
}

export async function desktopUpdatesSupported(): Promise<boolean> {
  if (backend.kind !== "native") return false;
  const { platform } = await backend.bootstrap();
  return ["macos", "windows", "linux"].includes(platform);
}

export async function checkForUpdate(): Promise<AvailableUpdate | null> {
  if (!(await desktopUpdatesSupported())) return null;
  const [currentVersion, update] = await Promise.all([getVersion(), check()]);
  if (update === null) return null;
  return wrapUpdate(update, currentVersion);
}

function wrapUpdate(update: Update, currentVersion: string): AvailableUpdate {
  return {
    info: {
      version: update.version,
      currentVersion,
      body: update.body ?? null,
      date: update.date ?? null,
    },
    downloadAndInstall: (onEvent) => update.downloadAndInstall(onEvent),
  };
}

export async function restartAfterUpdate(): Promise<void> {
  await relaunch();
}

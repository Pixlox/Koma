import { Files, LoaderCircle } from "lucide-react";
import { useEffect } from "react";
import { useTranslation } from "react-i18next";

import { AppShell } from "./components/AppShell";
import { CommandPalette } from "./components/CommandPalette";
import { DownloadTray } from "./components/DownloadTray";
import { ImportPanel } from "./components/ImportPanel";
import { PasswordDialog } from "./components/PasswordDialog";
import { Reader } from "./components/Reader";
import { Toasts } from "./components/Toasts";
import { ToolsPanel } from "./components/ToolsPanel";
import { backend } from "./lib/backend";
import { applyLanguage, tr } from "./i18n";
import { useKomaStore } from "./store/koma";

export default function App() {
  useTranslation();
  const initialized = useKomaStore((state) => state.initialized);
  const booting = useKomaStore((state) => state.booting);
  const initialize = useKomaStore((state) => state.initialize);
  const addFiles = useKomaStore((state) => state.addFiles);
  const setCommandOpen = useKomaStore((state) => state.setCommandOpen);
  const setImportOpen = useKomaStore((state) => state.setImportOpen);
  const setRoute = useKomaStore((state) => state.setRoute);
  const reader = useKomaStore((state) => state.reader);
  const dropActive = useKomaStore((state) => state.dropActive);
  const setDropActive = useKomaStore((state) => state.setDropActive);
  const importPaths = useKomaStore((state) => state.importPaths);
  const reloadLibrary = useKomaStore((state) => state.reloadLibrary);
  const reloadLibraryFolders = useKomaStore(
    (state) => state.reloadLibraryFolders,
  );
  const language = useKomaStore((state) => state.language);
  const platform = useKomaStore((state) => state.bootstrap?.platform);
  const route = useKomaStore((state) => state.route);

  useEffect(() => {
    void initialize();
  }, [initialize]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (platform === "ios" || platform === "android") return;
      const target = event.target;
      const isEditing =
        target instanceof HTMLInputElement ||
        target instanceof HTMLTextAreaElement ||
        target instanceof HTMLSelectElement ||
        (target instanceof HTMLElement && target.isContentEditable);
      const command = event.metaKey || event.ctrlKey;
      if (command && event.key.toLocaleLowerCase() === "k") {
        event.preventDefault();
        setCommandOpen(true);
      } else if (command && event.key.toLocaleLowerCase() === "o") {
        event.preventDefault();
        void addFiles();
      } else if (command && event.key === ",") {
        event.preventDefault();
        setRoute("settings");
      } else if (
        !isEditing &&
        !command &&
        event.key === "/" &&
        reader === null
      ) {
        event.preventDefault();
        document.querySelector<HTMLInputElement>("[data-koma-search]")?.focus();
      } else if (
        command &&
        event.shiftKey &&
        event.key.toLocaleLowerCase() === "i"
      ) {
        event.preventDefault();
        setImportOpen(true);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [addFiles, platform, reader, setCommandOpen, setImportOpen, setRoute]);

  useEffect(() => {
    if (!["macos", "windows", "linux"].includes(platform ?? "")) return;
    const enabled = localStorage.getItem("koma.discordPresence") === "true";
    const details =
      reader === null
        ? route === "home"
          ? tr("Browsing home")
          : tr("Browsing the library")
        : tr("Reading {{title}}", {
            title: reader.payload.manifest.metadata.title,
          });
    const activityState =
      reader === null
        ? tr("Choosing what to read")
        : tr("Page {{current}} of {{total}}", {
            current: reader.currentPage + 1,
            total: reader.payload.manifest.pages.length,
          });
    void backend
      .setDiscordPresence(enabled, details, activityState)
      .catch(() => undefined);
  }, [platform, reader, route]);

  useEffect(() => {
    if (backend.kind !== "native") return;
    let cancelled = false;
    const unlisteners: Array<() => void> = [];

    const rememberUnlistener = (unlisten: () => void) => {
      if (cancelled) unlisten();
      else unlisteners.push(unlisten);
    };

    void backend
      .onFileDrop((event) => {
        if (event.type === "over") {
          setDropActive(true);
        } else if (event.type === "drop") {
          setDropActive(false);
          void importPaths(event.paths);
        } else {
          setDropActive(false);
        }
      })
      .then(rememberUnlistener);

    void backend
      .onOpenPaths(() => {
        void backend.takeOpenPaths().then(importPaths);
      })
      .then(rememberUnlistener);

    return () => {
      cancelled = true;
      for (const unlisten of unlisteners) unlisten();
    };
  }, [importPaths, setDropActive]);

  useEffect(() => {
    if (language !== "system") return;
    const refresh = () => void applyLanguage("system");
    window.addEventListener("languagechange", refresh);
    return () => window.removeEventListener("languagechange", refresh);
  }, [language]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    void backend
      .onLibraryChanged(() => {
        void Promise.all([reloadLibrary(), reloadLibraryFolders()]);
      })
      .then((next) => {
        if (cancelled) next();
        else unlisten = next;
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [reloadLibrary, reloadLibraryFolders]);

  if (!initialized) {
    return (
      <div className="boot-screen">
        <img src="/koma-mark.svg" alt="" />
        <div>
          <strong>Koma</strong>
          <span>
            {booting ? tr("Opening library…") : tr("Preparing reader…")}
          </span>
        </div>
        <LoaderCircle className="spin" size={18} aria-label={tr("Loading")} />
      </div>
    );
  }

  return (
    <>
      <a className="skip-link" href="#main-content">
        {tr("Skip to library")}
      </a>
      <AppShell />
      <ImportPanel />
      <CommandPalette />
      <PasswordDialog />
      <ToolsPanel />
      <Reader />
      <DownloadTray />
      <Toasts />
      {dropActive && (
        <div className="drop-overlay" role="status" aria-live="polite">
          <div>
            <Files size={28} aria-hidden="true" />
            <strong>{tr("Drop to add to Koma")}</strong>
            <span>{tr("Add archives or folders.")}</span>
          </div>
        </div>
      )}
    </>
  );
}

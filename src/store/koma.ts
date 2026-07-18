import { create } from "zustand";

import { backend, errorCode, errorMessage } from "../lib/backend";
import { applyLanguage, isLanguageAvailable, tr } from "../i18n";
import { closePdf, renderPdfPage } from "../lib/pdf";
import type {
  Bookmark,
  BootstrapPayload,
  ConnectorSummary,
  LibraryItem,
  LibraryFolder,
  LibraryRoute,
  LibrarySortMode,
  LibraryViewMode,
  ReaderOpenPayload,
  ReaderSettings,
  LanguageMode,
  MotionMode,
  ThemeMode,
} from "../types";

export type ToastTone = "neutral" | "success" | "warning" | "danger";

export interface ToastMessage {
  id: number;
  title: string;
  detail: string | null;
  tone: ToastTone;
}

export interface ReaderRuntime {
  payload: ReaderOpenPayload;
  currentPage: number;
  settings: ReaderSettings;
  pageUrls: Record<number, string>;
  loadingPages: number[];
  bookmarks: Bookmark[];
  controlsVisible: boolean;
  settingsOpen: boolean;
  zoom: number;
  error: string | null;
  password: string | null;
  sourceUrl: string | null;
}

interface PasswordRequest {
  title: string;
  resolve: (password: string | null) => void;
}

interface KomaState {
  initialized: boolean;
  booting: boolean;
  bootstrap: BootstrapPayload | null;
  items: LibraryItem[];
  route: LibraryRoute;
  search: string;
  viewMode: LibraryViewMode;
  sortMode: LibrarySortMode;
  selectedId: string | null;
  theme: ThemeMode;
  language: LanguageMode;
  motion: MotionMode;
  libraryFolders: LibraryFolder[];
  connectors: ConnectorSummary[];
  reader: ReaderRuntime | null;
  readerOpeningId: string | null;
  importOpen: boolean;
  commandOpen: boolean;
  toolsItemId: string | null;
  sidebarOpen: boolean;
  dropActive: boolean;
  toasts: ToastMessage[];
  passwordRequest: PasswordRequest | null;
  initialize: () => Promise<void>;
  reloadLibrary: () => Promise<void>;
  setRoute: (route: LibraryRoute) => void;
  setSearch: (search: string) => void;
  setViewMode: (viewMode: LibraryViewMode) => void;
  setSortMode: (sortMode: LibrarySortMode) => void;
  setSelectedId: (selectedId: string | null) => void;
  setTheme: (theme: ThemeMode) => void;
  setLanguage: (language: LanguageMode) => void;
  setMotion: (motion: MotionMode) => void;
  setImportOpen: (open: boolean) => void;
  setCommandOpen: (open: boolean) => void;
  setToolsItemId: (itemId: string | null) => void;
  setSidebarOpen: (open: boolean) => void;
  setDropActive: (active: boolean) => void;
  importPaths: (paths: string[]) => Promise<void>;
  addFiles: () => Promise<void>;
  addFolder: () => Promise<void>;
  reloadLibraryFolders: () => Promise<void>;
  addManagedFolder: () => Promise<void>;
  updateManagedFolder: (
    folder: LibraryFolder,
    patch: Partial<Pick<LibraryFolder, "enabled" | "scanIntervalMinutes">>,
  ) => Promise<void>;
  removeManagedFolder: (folder: LibraryFolder) => Promise<void>;
  scanManagedFolder: (folder: LibraryFolder) => Promise<void>;
  reloadConnectors: () => Promise<void>;
  importConnector: () => Promise<void>;
  removeConnectorPackage: (connector: ConnectorSummary) => Promise<void>;
  setFavorite: (item: LibraryItem, favorite: boolean) => Promise<void>;
  setHidden: (item: LibraryItem, hidden: boolean) => Promise<void>;
  setCompleted: (item: LibraryItem, completed: boolean) => Promise<void>;
  removeItem: (item: LibraryItem) => Promise<void>;
  revealItem: (item: LibraryItem) => Promise<void>;
  relinkItem: (item: LibraryItem) => Promise<void>;
  openBook: (item: LibraryItem) => Promise<void>;
  closeReader: () => void;
  loadPage: (pageIndex: number) => Promise<void>;
  goToPage: (pageIndex: number) => Promise<void>;
  setReaderControls: (visible: boolean) => void;
  setReaderSettingsOpen: (open: boolean) => void;
  updateReaderSettings: (patch: Partial<ReaderSettings>) => Promise<void>;
  setReaderZoom: (zoom: number) => void;
  toggleReaderBookmark: () => Promise<void>;
  saveReaderAnnotation: (label: string, note: string) => Promise<void>;
  removeReaderBookmark: (bookmarkId: string) => Promise<void>;
  addImportedItem: (item: LibraryItem) => void;
  exportBackup: () => Promise<void>;
  restoreBackup: () => Promise<void>;
  requestPassword: (title: string) => Promise<string | null>;
  notify: (title: string, detail?: string, tone?: ToastTone) => void;
  dismissToast: (id: number) => void;
  resolvePassword: (password: string | null) => void;
}

let nextToastId = 1;
const MAX_CACHED_READER_PAGES = 28;

function savedViewMode(): LibraryViewMode {
  return localStorage.getItem("koma.viewMode") === "list" ? "list" : "grid";
}

function savedTheme(): ThemeMode {
  const value = localStorage.getItem("koma.theme");
  return value === "light" || value === "dark" ? value : "system";
}

function savedLanguage(): LanguageMode {
  const value = localStorage.getItem("koma.language");
  return value !== null && isLanguageAvailable(value) ? value : "system";
}

function savedMotion(): MotionMode {
  const value = localStorage.getItem("koma.motion");
  return value === "on" || value === "off" ? value : "system";
}

function savedSortMode(): LibrarySortMode {
  const value = localStorage.getItem("koma.sortMode");
  return value === "title" ||
    value === "series" ||
    value === "added" ||
    value === "progress"
    ? value
    : "recent";
}

function applyTheme(theme: ThemeMode): void {
  document.documentElement.dataset.theme = theme;
  document.documentElement.style.colorScheme =
    theme === "system" ? "light dark" : theme;
}

function applyMotion(motion: MotionMode): void {
  document.documentElement.dataset.motion = motion;
}

function replaceItem(items: LibraryItem[], next: LibraryItem): LibraryItem[] {
  return items.map((item) => (item.id === next.id ? next : item));
}

function mergeItem(items: LibraryItem[], id: string, patch: Partial<LibraryItem>) {
  return items.map((item) => (item.id === id ? { ...item, ...patch } : item));
}

function cacheReaderPage(
  urls: Record<number, string>,
  pageIndex: number,
  dataUrl: string,
  currentPage: number,
): Record<number, string> {
  const next = { ...urls, [pageIndex]: dataUrl };
  const indexes = Object.keys(next).map(Number);
  if (indexes.length <= MAX_CACHED_READER_PAGES) return next;
  indexes
    .sort(
      (left, right) =>
        Math.abs(right - currentPage) - Math.abs(left - currentPage),
    )
    .slice(0, indexes.length - MAX_CACHED_READER_PAGES)
    .forEach((index) => {
      delete next[index];
    });
  return next;
}

export const useKomaStore = create<KomaState>((set, get) => ({
  initialized: false,
  booting: false,
  bootstrap: null,
  items: [],
  route: "home",
  search: "",
  viewMode: savedViewMode(),
  sortMode: savedSortMode(),
  selectedId: null,
  theme: savedTheme(),
  language: savedLanguage(),
  motion: savedMotion(),
  libraryFolders: [],
  connectors: [],
  reader: null,
  readerOpeningId: null,
  importOpen: false,
  commandOpen: false,
  toolsItemId: null,
  sidebarOpen: false,
  dropActive: false,
  toasts: [],
  passwordRequest: null,

  initialize: async () => {
    if (get().initialized || get().booting) return;
    set({ booting: true });
    applyTheme(get().theme);
    applyMotion(get().motion);
    await applyLanguage(get().language);
    let payload: BootstrapPayload;
    try {
      payload = await backend.bootstrap();
    } catch (error) {
      set({ booting: false });
      get().notify(
        tr("Koma could not open the library"),
        errorMessage(error),
        "danger",
      );
      return;
    }
    document.documentElement.dataset.platform = payload.platform;

    const [libraryFolders, connectors] = await Promise.all([
      backend.listLibraryFolders().catch(() => []),
      backend.listConnectors().catch(() => []),
    ]);
    set({
      initialized: true,
      booting: false,
      bootstrap: payload,
      items: payload.items,
      libraryFolders,
      connectors,
      selectedId: payload.items[0]?.id ?? null,
    });

    try {
      const pendingPaths = await backend.takeOpenPaths();
      if (pendingPaths.length > 0) {
        await get().importPaths(pendingPaths);
      }
    } catch (error) {
      get().notify(
        tr("Requested files could not be opened"),
        errorMessage(error),
        "danger",
      );
    }
  },

  reloadLibrary: async () => {
    try {
      const items = await backend.listLibrary(true);
      set({ items });
    } catch (error) {
      get().notify(tr("Library refresh failed"), errorMessage(error), "danger");
    }
  },

  setRoute: (route) => set({ route, sidebarOpen: false }),
  setSearch: (search) => set({ search }),
  setViewMode: (viewMode) => {
    localStorage.setItem("koma.viewMode", viewMode);
    set({ viewMode });
  },
  setSortMode: (sortMode) => {
    localStorage.setItem("koma.sortMode", sortMode);
    set({ sortMode });
  },
  setSelectedId: (selectedId) => set({ selectedId }),
  setTheme: (theme) => {
    localStorage.setItem("koma.theme", theme);
    applyTheme(theme);
    set({ theme });
  },
  setLanguage: (language) => {
    localStorage.setItem("koma.language", language);
    set({ language });
    void applyLanguage(language);
  },
  setMotion: (motion) => {
    localStorage.setItem("koma.motion", motion);
    applyMotion(motion);
    set({ motion });
  },
  setImportOpen: (importOpen) => set({ importOpen }),
  setCommandOpen: (commandOpen) => set({ commandOpen }),
  setToolsItemId: (toolsItemId) => set({ toolsItemId }),
  setSidebarOpen: (sidebarOpen) => set({ sidebarOpen }),
  setDropActive: (dropActive) => set({ dropActive }),

  importPaths: async (inputPaths) => {
    const paths = [...new Set(inputPaths.map((path) => path.trim()))].filter(
      Boolean,
    );
    if (paths.length === 0) return;

    let imported = 0;
    let failed = 0;
    for (const path of paths) {
      try {
        const item = await backend.addPublication(path);
        set((state) => ({
          items: [
            item,
            ...state.items.filter((candidate) => candidate.id !== item.id),
          ],
          selectedId: item.id,
          route: "library",
        }));
        imported += 1;
      } catch (error) {
        const code = errorCode(error);
        if (code === "empty_publication" || code === "unsupported_format") {
          try {
            const report = await backend.scanFolder(path);
            if (report.imported.length > 0) {
              set((state) => {
                const ids = new Set(report.imported.map((item) => item.id));
                return {
                  items: [
                    ...report.imported,
                    ...state.items.filter((item) => !ids.has(item.id)),
                  ],
                  selectedId: report.imported[0]?.id ?? state.selectedId,
                  route: "library",
                };
              });
              imported += report.imported.length;
              failed += report.failures.length;
              continue;
            }
          } catch {
            // Preserve the original, more specific publication error below.
          }
        }
        failed += 1;
        get().notify(
          tr("Could not add {{name}}", {
            name: path.split(/[\\/]/).pop() ?? tr("publication"),
          }),
          errorMessage(error),
          "warning",
        );
      }
    }
    if (imported > 0) {
      get().notify(
        tr("{{count}} publications added", { count: imported }),
        failed > 0
          ? tr("{{count}} items could not be added", { count: failed })
          : undefined,
        failed > 0 ? "warning" : "success",
      );
    }
  },

  addFiles: async () => {
    try {
      const paths = await backend.pickPublications();
      if (paths.length === 0) {
        if (backend.kind === "preview") {
          get().notify(
            tr("File picker"),
            tr("Available in the desktop app."),
            "neutral",
          );
        }
        return;
      }
      await get().importPaths(paths);
    } catch (error) {
      get().notify(tr("File picker failed"), errorMessage(error), "danger");
    }
  },

  addFolder: async () => {
    try {
      const path = await backend.pickFolder();
      if (path === null) {
        if (backend.kind === "preview") {
          get().notify(
            tr("Folder scan"),
            tr("Available in the desktop app."),
          );
        }
        return;
      }
      get().notify(tr("Scanning folder"), path);
      const report = await backend.scanFolder(path);
      set((state) => {
        const importedIds = new Set(report.imported.map((item) => item.id));
        return {
          items: [
            ...report.imported,
            ...state.items.filter((item) => !importedIds.has(item.id)),
          ],
          selectedId: report.imported[0]?.id ?? state.selectedId,
        };
      });
      get().notify(
        tr("{{count}} publications found", {
          count: report.imported.length,
        }),
        report.failures.length > 0
          ? tr("{{count}} items need attention", {
              count: report.failures.length,
            })
          : undefined,
        report.failures.length > 0 ? "warning" : "success",
      );
    } catch (error) {
      get().notify(tr("Folder scan failed"), errorMessage(error), "danger");
    }
  },

  reloadLibraryFolders: async () => {
    try {
      set({ libraryFolders: await backend.listLibraryFolders() });
    } catch (error) {
      get().notify(tr("Folder list could not be loaded"), errorMessage(error), "warning");
    }
  },

  addManagedFolder: async () => {
    try {
      const path = await backend.pickFolder();
      if (path === null) {
        if (backend.kind === "preview") {
          get().notify(tr("Folder picker"), tr("Available in the desktop app."));
        }
        return;
      }
      get().notify(tr("Scanning folder"), path);
      const result = await backend.addLibraryFolder(path, 60);
      set((state) => {
        const importedIds = new Set(result.report.imported.map((item) => item.id));
        return {
          libraryFolders: [
            result.folder,
            ...state.libraryFolders.filter((folder) => folder.id !== result.folder.id),
          ],
          items: [
            ...result.report.imported,
            ...state.items.filter((item) => !importedIds.has(item.id)),
          ],
        };
      });
      get().notify(
        tr("Folder added"),
        tr("{{count}} new or changed publications", {
          count: result.report.imported.length,
        }),
        result.report.failures.length > 0 ? "warning" : "success",
      );
    } catch (error) {
      get().notify(tr("Folder could not be added"), errorMessage(error), "danger");
    }
  },

  updateManagedFolder: async (folder, patch) => {
    const previous = folder;
    const optimistic = { ...folder, ...patch };
    set((state) => ({
      libraryFolders: state.libraryFolders.map((candidate) =>
        candidate.id === folder.id ? optimistic : candidate,
      ),
    }));
    try {
      const updated = await backend.updateLibraryFolder(
        folder.id,
        optimistic.enabled,
        optimistic.scanIntervalMinutes,
      );
      set((state) => ({
        libraryFolders: state.libraryFolders.map((candidate) =>
          candidate.id === folder.id ? updated : candidate,
        ),
      }));
    } catch (error) {
      set((state) => ({
        libraryFolders: state.libraryFolders.map((candidate) =>
          candidate.id === folder.id ? previous : candidate,
        ),
      }));
      get().notify(tr("Folder setting was not saved"), errorMessage(error), "danger");
    }
  },

  removeManagedFolder: async (folder) => {
    try {
      if (!(await backend.removeLibraryFolder(folder.id))) return;
      set((state) => ({
        libraryFolders: state.libraryFolders.filter(
          (candidate) => candidate.id !== folder.id,
        ),
      }));
      get().notify(tr("Folder removed"), tr("Library entries were kept."), "success");
    } catch (error) {
      get().notify(tr("Folder could not be removed"), errorMessage(error), "danger");
    }
  },

  scanManagedFolder: async (folder) => {
    try {
      const result = await backend.scanLibraryFolder(folder.id);
      const importedIds = new Set(result.report.imported.map((item) => item.id));
      set((state) => ({
        libraryFolders: state.libraryFolders.map((candidate) =>
          candidate.id === folder.id ? result.folder : candidate,
        ),
        items: [
          ...result.report.imported,
          ...state.items.filter((item) => !importedIds.has(item.id)),
        ],
      }));
      get().notify(
        tr("Scan complete"),
        tr("{{count}} new or changed publications", {
          count: result.report.imported.length,
        }),
        result.report.failures.length > 0 ? "warning" : "success",
      );
    } catch (error) {
      await get().reloadLibraryFolders();
      get().notify(tr("Folder scan failed"), errorMessage(error), "danger");
    }
  },

  reloadConnectors: async () => {
    try {
      set({ connectors: await backend.listConnectors() });
    } catch (error) {
      get().notify(
        tr("Connectors could not be loaded"),
        errorMessage(error),
        "warning",
      );
    }
  },

  importConnector: async () => {
    try {
      const path = await backend.pickConnectorPackage();
      if (path === null) {
        if (backend.kind === "preview") {
          get().notify(tr("Connector import"), tr("Available in the desktop app."));
        }
        return;
      }
      const preview = await backend.inspectConnectorPackage(path);
      const hosts = [
        ...new Set([
          ...preview.allowedRequestHosts,
          ...preview.allowedPageHosts,
        ]),
      ].join(", ");
      const localNetwork = preview.allowLocalNetwork
        ? `\n${tr("Local network access")}`
        : "";
      const accepted = window.confirm(
        `${tr("Install {{name}}?", { name: preview.connector.name })}\n\n${tr(
          "Network access: {{hosts}}",
          { hosts },
        )}${localNetwork}`,
      );
      if (!accepted) return;
      const connector = await backend.installConnectorPackage(path);
      set((state) => ({
        connectors: [
          ...state.connectors.filter((candidate) => candidate.id !== connector.id),
          connector,
        ].sort((left, right) => left.name.localeCompare(right.name)),
      }));
      get().notify(
        tr("Connector installed"),
        tr("{{name}} is ready.", { name: connector.name }),
        "success",
      );
    } catch (error) {
      get().notify(
        tr("Connector could not be installed"),
        errorMessage(error),
        "danger",
      );
    }
  },

  removeConnectorPackage: async (connector) => {
    if (!connector.removable) return;
    if (
      !window.confirm(
        tr("Remove {{name}}?", {
          name: connector.name,
        }),
      )
    ) {
      return;
    }
    try {
      if (!(await backend.removeConnector(connector.id))) return;
      set((state) => ({
        connectors: state.connectors.filter(
          (candidate) => candidate.id !== connector.id,
        ),
      }));
      get().notify(tr("Connector removed"), connector.name, "success");
    } catch (error) {
      get().notify(
        tr("Connector could not be removed"),
        errorMessage(error),
        "danger",
      );
    }
  },

  setFavorite: async (item, favorite) => {
    set((state) => ({
      items: mergeItem(state.items, item.id, { isFavorite: favorite }),
    }));
    try {
      await backend.setFavorite(item.id, favorite);
    } catch (error) {
      set((state) => ({ items: replaceItem(state.items, item) }));
      get().notify(
        tr("Could not update favorite"),
        errorMessage(error),
        "danger",
      );
    }
  },

  setHidden: async (item, hidden) => {
    set((state) => ({
      items: mergeItem(state.items, item.id, { isHidden: hidden }),
      selectedId:
        hidden && state.route !== "hidden" && state.selectedId === item.id
          ? null
          : state.selectedId,
    }));
    try {
      await backend.setHidden(item.id, hidden);
      get().notify(
        tr(hidden ? "Moved to Hidden" : "Restored to Library"),
        item.title,
        "success",
      );
    } catch (error) {
      set((state) => ({ items: replaceItem(state.items, item) }));
      get().notify(
        tr("Could not change visibility"),
        errorMessage(error),
        "danger",
      );
    }
  },

  setCompleted: async (item, completed) => {
    const targetPage = completed ? Math.max(0, item.pageCount - 1) : 0;
    set((state) => ({
      items: mergeItem(state.items, item.id, {
        currentPage: targetPage,
        progress: completed ? 1 : 0,
        isCompleted: completed,
      }),
    }));
    try {
      const reading = await backend.setReadingStatus(item.id, completed);
      set((state) => ({
        items: mergeItem(state.items, item.id, {
          currentPage: reading.currentPage,
          progress: reading.progress,
          isCompleted: reading.completed,
        }),
      }));
      get().notify(
        tr(completed ? "Marked as read" : "Marked as unread"),
        item.title,
        "success",
      );
    } catch (error) {
      set((state) => ({ items: replaceItem(state.items, item) }));
      get().notify(
        tr("Could not update reading status"),
        errorMessage(error),
        "danger",
      );
    }
  },

  removeItem: async (item) => {
    try {
      const removed = await backend.removeFromLibrary(item.id);
      if (!removed) return;
      set((state) => ({
        items: state.items.filter((candidate) => candidate.id !== item.id),
        selectedId: state.selectedId === item.id ? null : state.selectedId,
      }));
      get().notify(
        tr("Removed from Koma"),
        tr("Original file kept."),
        "success",
      );
    } catch (error) {
      get().notify(
        tr("Could not remove publication"),
        errorMessage(error),
        "danger",
      );
    }
  },

  revealItem: async (item) => {
    try {
      await backend.revealItem(item.path);
      if (backend.kind === "preview") {
        get().notify(
          tr("Reveal in folder"),
          tr("Available in the desktop app."),
        );
      }
    } catch (error) {
      get().notify(
        tr("Could not reveal the file"),
        errorMessage(error),
        "warning",
      );
    }
  },

  relinkItem: async (item) => {
    try {
      const path = await backend.pickRelinkSource();
      if (path === null) return;
      let password: string | null = null;
      for (;;) {
        try {
          const relinked = await backend.relinkPublication(
            item.id,
            path,
            password ?? undefined,
          );
          set((state) => ({
            items: replaceItem(state.items, relinked),
            selectedId: relinked.id,
          }));
          get().notify(
            tr("Source relinked"),
            item.title,
            "success",
          );
          return;
        } catch (error) {
          if (errorCode(error) !== "password_required") throw error;
          password = await get().requestPassword(item.title);
          if (password === null) return;
        }
      }
    } catch (error) {
      get().notify(
        tr("Could not relink source"),
        errorMessage(error),
        "danger",
      );
    }
  },

  openBook: async (item) => {
    if (get().readerOpeningId !== null) return;
    set({ readerOpeningId: item.id });
    try {
      let password: string | null = null;
      let payload: ReaderOpenPayload;
      for (;;) {
        try {
          payload = await backend.openReader(item.id, password ?? undefined);
          break;
        } catch (error) {
          if (errorCode(error) !== "password_required") throw error;
          password = await get().requestPassword(item.title);
          if (password === null) {
            set({ readerOpeningId: null });
            return;
          }
        }
      }
      const currentPage = Math.min(
        payload.manifest.pages.length - 1,
        Math.max(0, payload.readingState?.currentPage ?? item.currentPage),
      );
      const sourceUrl =
        payload.manifest.format === "pdf"
          ? backend.publicationSourceUrl(payload.manifest.path)
          : null;
      const page =
        sourceUrl === null
          ? await backend.readPage(item.id, currentPage)
          : await renderPdfPage(sourceUrl, currentPage, password);
      const settings =
        payload.readingState?.settings ??
        get().bootstrap?.defaultReaderSettings ?? {
          mode: "singlePage",
          direction: "automatic",
          fit: "smart",
          widePagePolicy: "keep",
          cropMargins: false,
          gapPx: 12,
          spreadGapEnabled: true,
          brightness: 1,
          contrast: 1,
          saturation: 1,
          gamma: 1,
          grayscale: false,
          invert: false,
          sharpen: false,
          keepAwake: true,
          showPageNumber: true,
        };
      set({
        readerOpeningId: null,
        reader: {
          payload,
          currentPage,
          settings,
          pageUrls: { [currentPage]: page.dataUrl },
          loadingPages: [],
          bookmarks: payload.bookmarks,
          controlsVisible: true,
          settingsOpen: false,
          zoom: 1,
          error: null,
          password,
          sourceUrl,
        },
      });
      const next = currentPage + 1;
      if (next < payload.manifest.pages.length) {
        void get().loadPage(next);
      }
    } catch (error) {
      set({ readerOpeningId: null });
      const code = errorCode(error);
      get().notify(
        tr(
          code === "password_required"
            ? "Password required"
            : "Could not open publication",
        ),
        errorMessage(error),
        code === "password_required" ? "warning" : "danger",
      );
    }
  },

  closeReader: () => {
    const sourceUrl = get().reader?.sourceUrl;
    if (sourceUrl !== null && sourceUrl !== undefined) closePdf(sourceUrl);
    set({ reader: null });
    void get().reloadLibrary();
  },

  loadPage: async (pageIndex) => {
    const reader = get().reader;
    if (
      reader === null ||
      reader.pageUrls[pageIndex] !== undefined ||
      reader.loadingPages.includes(pageIndex) ||
      pageIndex < 0 ||
      pageIndex >= reader.payload.manifest.pages.length
    ) {
      return;
    }
    set({
      reader: {
        ...reader,
        loadingPages: [...reader.loadingPages, pageIndex],
      },
    });
    try {
      const page =
        reader.sourceUrl === null
          ? await backend.readPage(reader.payload.libraryId, pageIndex)
          : await renderPdfPage(reader.sourceUrl, pageIndex, reader.password);
      set((state) => {
        if (state.reader?.payload.libraryId !== reader.payload.libraryId) {
          return {};
        }
        return {
          reader: {
            ...state.reader,
            pageUrls: cacheReaderPage(
              state.reader.pageUrls,
              pageIndex,
              page.dataUrl,
              state.reader.currentPage,
            ),
            loadingPages: state.reader.loadingPages.filter(
              (candidate) => candidate !== pageIndex,
            ),
          },
        };
      });
    } catch (error) {
      set((state) => {
        if (state.reader === null) return {};
        return {
          reader: {
            ...state.reader,
            loadingPages: state.reader.loadingPages.filter(
              (candidate) => candidate !== pageIndex,
            ),
            error: errorMessage(error),
          },
        };
      });
    }
  },

  goToPage: async (pageIndex) => {
    const reader = get().reader;
    if (reader === null) return;
    const bounded = Math.max(
      0,
      Math.min(reader.payload.manifest.pages.length - 1, pageIndex),
    );
    set({
      reader: {
        ...reader,
        currentPage: bounded,
        controlsVisible: reader.controlsVisible,
        error: null,
      },
    });
    await get().loadPage(bounded);
    const updatedReader = get().reader;
    if (updatedReader === null) return;
    try {
      const state = await backend.saveProgress(
        updatedReader.payload.libraryId,
        bounded,
        updatedReader.settings,
      );
      set((current) => ({
        items: mergeItem(current.items, updatedReader.payload.libraryId, {
          currentPage: state.currentPage,
          progress: state.progress,
          isCompleted: state.completed,
          lastOpenedAt: state.updatedAt,
        }),
      }));
    } catch (error) {
      get().notify(
        tr("Progress was not saved"),
        errorMessage(error),
        "warning",
      );
    }
    for (const nearby of [bounded - 1, bounded + 1, bounded + 2]) {
      void get().loadPage(nearby);
    }
  },

  setReaderControls: (visible) => {
    const reader = get().reader;
    if (reader !== null) set({ reader: { ...reader, controlsVisible: visible } });
  },

  setReaderSettingsOpen: (open) => {
    const reader = get().reader;
    if (reader !== null) {
      set({
        reader: {
          ...reader,
          settingsOpen: open,
          controlsVisible: true,
        },
      });
    }
  },

  updateReaderSettings: async (patch) => {
    const reader = get().reader;
    if (reader === null) return;
    const settings = { ...reader.settings, ...patch };
    set({ reader: { ...reader, settings } });
    try {
      await backend.saveProgress(
        reader.payload.libraryId,
        reader.currentPage,
        settings,
      );
    } catch (error) {
      get().notify(
        tr("Reader setting was not saved"),
        errorMessage(error),
        "warning",
      );
    }
  },

  setReaderZoom: (zoom) => {
    const reader = get().reader;
    if (reader !== null) {
      set({
        reader: {
          ...reader,
          zoom: Math.max(0.25, Math.min(5, zoom)),
        },
      });
    }
  },

  toggleReaderBookmark: async () => {
    const reader = get().reader;
    if (reader === null) return;
    const existing = reader.bookmarks.find(
      (bookmark) => bookmark.pageIndex === reader.currentPage,
    );
    try {
      if (existing !== undefined) {
        await backend.removeBookmark(existing.id);
        const current = get().reader;
        if (current !== null) {
          set({
            reader: {
              ...current,
              bookmarks: current.bookmarks.filter(
                (bookmark) => bookmark.id !== existing.id,
              ),
            },
          });
        }
      } else {
        const bookmark = await backend.addBookmark(
          reader.payload.libraryId,
          reader.currentPage,
          tr("Page {{page}}", { page: reader.currentPage + 1 }),
        );
        const current = get().reader;
        if (current !== null) {
          set({
            reader: {
              ...current,
              bookmarks: [...current.bookmarks, bookmark],
            },
          });
        }
      }
    } catch (error) {
      get().notify(
        tr("Bookmark was not changed"),
        errorMessage(error),
        "warning",
      );
    }
  },

  saveReaderAnnotation: async (label, note) => {
    const reader = get().reader;
    if (reader === null) return;
    const existing = reader.bookmarks.find(
      (bookmark) => bookmark.pageIndex === reader.currentPage,
    );
    try {
      const bookmark =
        existing === undefined
          ? await backend.addBookmark(
              reader.payload.libraryId,
              reader.currentPage,
              label,
              note,
            )
          : await backend.updateBookmark(existing.id, label, note);
      const current = get().reader;
      if (current === null) return;
      set({
        reader: {
          ...current,
          bookmarks:
            existing === undefined
              ? [...current.bookmarks, bookmark]
              : current.bookmarks.map((candidate) =>
                  candidate.id === existing.id
                    ? {
                        ...bookmark,
                        publicationId: existing.publicationId,
                        pageIndex: existing.pageIndex,
                        createdAt: existing.createdAt,
                      }
                    : candidate,
                ),
        },
      });
      get().notify(
        tr("Page note saved"),
        tr("Page {{page}}", { page: reader.currentPage + 1 }),
        "success",
      );
    } catch (error) {
      get().notify(
        tr("Page note was not saved"),
        errorMessage(error),
        "warning",
      );
    }
  },

  removeReaderBookmark: async (bookmarkId) => {
    const reader = get().reader;
    if (reader === null) return;
    try {
      await backend.removeBookmark(bookmarkId);
      const current = get().reader;
      if (current !== null) {
        set({
          reader: {
            ...current,
            bookmarks: current.bookmarks.filter(
              (bookmark) => bookmark.id !== bookmarkId,
            ),
          },
        });
      }
    } catch (error) {
      get().notify(
        tr("Bookmark was not removed"),
        errorMessage(error),
        "warning",
      );
    }
  },

  addImportedItem: (item) => {
    set((state) => ({
      items: [item, ...state.items.filter((candidate) => candidate.id !== item.id)],
      selectedId: item.id,
      route: "library",
    }));
  },

  exportBackup: async () => {
    try {
      const destination = await backend.pickBackupDestination();
      if (destination === null) {
        if (backend.kind === "preview") {
          get().notify(
            tr("Backup export"),
            tr("Available in the desktop app."),
          );
        }
        return;
      }
      const output = await backend.exportLibraryBackup(destination);
      get().notify(tr("Library backup saved"), output, "success");
    } catch (error) {
      get().notify(tr("Backup failed"), errorMessage(error), "danger");
    }
  },

  restoreBackup: async () => {
    try {
      const source = await backend.pickBackupSource();
      if (source === null) {
        if (backend.kind === "preview") {
          get().notify(
            tr("Backup restore"),
            tr("Available in the desktop app."),
          );
        }
        return;
      }
      if (
        !window.confirm(
          tr("Merge this backup into Koma? Current library entries will be kept."),
        )
      ) {
        return;
      }
      const report = await backend.restoreLibraryBackup(source);
      await get().reloadLibrary();
      get().notify(
        tr("Library backup restored"),
        `${tr("{{count}} publications", {
          count: report.publications,
        })} · ${tr("{{count}} bookmarks", {
          count: report.bookmarks,
        })}${
          report.missingSources > 0
            ? ` · ${tr("{{count}} sources missing", {
                count: report.missingSources,
              })}`
            : ""
        }`,
        report.missingSources > 0 ? "warning" : "success",
      );
    } catch (error) {
      get().notify(tr("Restore failed"), errorMessage(error), "danger");
    }
  },

  requestPassword: (title) =>
    new Promise<string | null>((resolve) => {
      set({ passwordRequest: { title, resolve } });
    }),

  notify: (title, detail, tone = "neutral") => {
    const id = nextToastId++;
    set((state) => ({
      toasts: [
        ...state.toasts,
        { id, title, detail: detail ?? null, tone },
      ].slice(-4),
    }));
    window.setTimeout(() => {
      get().dismissToast(id);
    }, 5_000);
  },

  dismissToast: (id) =>
    set((state) => ({
      toasts: state.toasts.filter((toast) => toast.id !== id),
    })),

  resolvePassword: (password) => {
    const request = get().passwordRequest;
    if (request === null) return;
    set({ passwordRequest: null });
    request.resolve(password);
  },
}));

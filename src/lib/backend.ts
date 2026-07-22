import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open, save } from "@tauri-apps/plugin-dialog";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";

import { locale, tr } from "../i18n";
import {
  DEFAULT_READER_SETTINGS,
  demoBootstrap,
  demoOpenPayload,
  demoPagePayload,
  readDemoItems,
  writeDemoItems,
} from "./demo";
import type {
  Bookmark,
  BackupRestoreReport,
  BootstrapPayload,
  CommandError,
  ConversionOptions,
  ConnectorPackagePreview,
  ConnectorSummary,
  ImportEvent,
  ImportEventPayload,
  ImportOptions,
  ImportPreview,
  LibraryItem,
  LibraryFolder,
  LibraryFolderScanResult,
  LibraryScanReport,
  LinkImportResult,
  MetadataSaveResult,
  OnlineNavigationResult,
  PagePayload,
  PublicationInspection,
  PublicationMetadata,
  PublicationOperationResult,
  ReaderOpenPayload,
  ReaderSettings,
  ReadingState,
  TrackingAccount,
  TrackingAuthEvent,
  TrackingMapping,
  TrackingRemoteProgress,
  TrackingProvider,
  TrackingSuggestion,
} from "../types";

export interface KomaBackend {
  readonly kind: "native" | "preview";
  bootstrap(): Promise<BootstrapPayload>;
  listLibrary(includeHidden: boolean, search?: string): Promise<LibraryItem[]>;
  takeOpenPaths(): Promise<string[]>;
  onOpenPaths(handler: () => void): Promise<UnlistenFn>;
  onLibraryChanged(handler: () => void): Promise<UnlistenFn>;
  onFileDrop(handler: (event: FileDropState) => void): Promise<UnlistenFn>;
  pickPublicationUploads(): Promise<File[]>;
  uploadPublication(file: File, password?: string): Promise<LibraryItem>;
  pickPublications(): Promise<string[]>;
  pickRelinkSource(): Promise<string | null>;
  pickFolder(): Promise<string | null>;
  pickConnectorPackage(): Promise<string | null>;
  pickConnectorUpload(): Promise<File | null>;
  inspectConnectorUpload(file: File): Promise<ConnectorPackagePreview>;
  pickBackupDestination(): Promise<string | null>;
  pickBackupSource(): Promise<string | null>;
  pickCbzDestination(suggestedName: string): Promise<string | null>;
  addPublication(path: string, password?: string): Promise<LibraryItem>;
  relinkPublication(
    publicationId: string,
    path: string,
    password?: string,
  ): Promise<LibraryItem>;
  scanFolder(path: string): Promise<LibraryScanReport>;
  listLibraryFolders(): Promise<LibraryFolder[]>;
  addLibraryFolder(
    path: string,
    scanIntervalMinutes: number,
  ): Promise<LibraryFolderScanResult>;
  updateLibraryFolder(
    folderId: string,
    enabled: boolean,
    scanIntervalMinutes: number,
  ): Promise<LibraryFolder>;
  removeLibraryFolder(folderId: string): Promise<boolean>;
  scanLibraryFolder(folderId: string): Promise<LibraryFolderScanResult>;
  listConnectors(): Promise<ConnectorSummary[]>;
  inspectConnectorPackage(path: string): Promise<ConnectorPackagePreview>;
  installConnectorPackage(path: string): Promise<ConnectorSummary>;
  removeConnector(connectorId: string): Promise<boolean>;
  removeFromLibrary(publicationId: string): Promise<boolean>;
  setHidden(publicationId: string, hidden: boolean): Promise<boolean>;
  setFavorite(publicationId: string, favorite: boolean): Promise<boolean>;
  setReadingStatus(
    publicationId: string,
    completed: boolean,
  ): Promise<ReadingState>;
  openReader(
    publicationId: string,
    password?: string,
  ): Promise<ReaderOpenPayload>;
  readPage(publicationId: string, pageIndex: number): Promise<PagePayload>;
  saveProgress(
    publicationId: string,
    currentPage: number,
    settings?: ReaderSettings,
  ): Promise<ReadingState>;
  recordReadingTime(
    publicationId: string,
    elapsedSeconds: number,
  ): Promise<number>;
  trackingAccounts(): Promise<TrackingAccount[]>;
  beginTrackingOAuth(provider: TrackingProvider): Promise<void>;
  takeTrackingAuth(): Promise<TrackingAuthEvent | null>;
  onTrackingAuth(
    handler: (event: TrackingAuthEvent) => void,
  ): Promise<UnlistenFn>;
  disconnectTracking(provider: TrackingProvider): Promise<void>;
  suggestTracking(
    provider: TrackingProvider,
    query: string,
  ): Promise<TrackingSuggestion>;
  trackingMappings(publicationId: string): Promise<TrackingMapping[]>;
  setTrackingMapping(mapping: TrackingMapping): Promise<void>;
  removeTrackingMapping(
    publicationId: string,
    provider: TrackingProvider,
  ): Promise<void>;
  trackingRemoteProgress(
    publicationId: string,
  ): Promise<TrackingRemoteProgress[]>;
  addBookmark(
    publicationId: string,
    pageIndex: number,
    label?: string,
    note?: string,
  ): Promise<Bookmark>;
  updateBookmark(
    bookmarkId: string,
    label?: string,
    note?: string,
  ): Promise<Bookmark>;
  removeBookmark(bookmarkId: string): Promise<boolean>;
  previewLink(source: string): Promise<ImportPreview>;
  importLink(source: string, options: ImportOptions): Promise<LinkImportResult>;
  readLinkOnline(source: string, options: ImportOptions): Promise<LibraryItem>;
  navigateOnlinePublication(
    publicationId: string,
    targetScope: "chapter" | "volume",
    targetId: number,
  ): Promise<OnlineNavigationResult>;
  downloadOnlinePublication(
    publicationId: string,
    options?: ImportOptions,
  ): Promise<LinkImportResult>;
  onImportEvent(
    handler: (event: ImportEvent, jobId: string) => void,
  ): Promise<UnlistenFn>;
  cancelImport(jobId: string): Promise<boolean>;
  setDiscordPresence(
    enabled: boolean,
    details: string,
    activityState: string,
  ): Promise<boolean>;
  exportLibraryBackup(destination: string): Promise<string>;
  restoreLibraryBackup(source: string): Promise<BackupRestoreReport>;
  inspectPublication(
    publicationId: string,
    password?: string,
  ): Promise<PublicationInspection>;
  convertPublication(
    publicationId: string,
    destination: string,
    options: ConversionOptions,
    password?: string,
  ): Promise<PublicationOperationResult>;
  repairPublication(
    publicationId: string,
    destination: string,
    password?: string,
  ): Promise<PublicationOperationResult>;
  saveMetadata(
    publicationId: string,
    metadata: PublicationMetadata,
    writeToSource: boolean,
  ): Promise<MetadataSaveResult>;
  revealItem(path: string): Promise<void>;
  publicationSourceUrl(path: string): string;
}

export type FileDropState =
  { type: "over" } | { type: "drop"; paths: string[] } | { type: "cancel" };

function pickBrowserFiles(accept: string, multiple: boolean): Promise<File[]> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = accept;
    input.multiple = multiple;
    input.style.display = "none";
    document.body.append(input);
    let settled = false;
    const finish = (files: File[]) => {
      if (settled) return;
      settled = true;
      input.remove();
      resolve(files);
    };
    input.addEventListener(
      "change",
      () => finish(Array.from(input.files ?? [])),
      { once: true },
    );
    input.addEventListener("cancel", () => finish([]), { once: true });
    input.click();
  });
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  const stride = 32 * 1024;
  for (let offset = 0; offset < bytes.length; offset += stride) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + stride));
  }
  return btoa(binary);
}

class NativeBackend implements KomaBackend {
  readonly kind = "native" as const;

  bootstrap() {
    return invoke<BootstrapPayload>("bootstrap");
  }

  listLibrary(includeHidden: boolean, search?: string) {
    return invoke<LibraryItem[]>("list_library", {
      includeHidden,
      search: search ?? null,
    });
  }

  takeOpenPaths() {
    return invoke<string[]>("take_open_paths");
  }

  onOpenPaths(handler: () => void) {
    return listen("koma://open-paths", handler);
  }

  onLibraryChanged(handler: () => void) {
    return listen("koma://library-changed", handler);
  }

  onFileDrop(handler: (event: FileDropState) => void) {
    return getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === "drop") {
        handler({ type: "drop", paths: event.payload.paths });
      } else if (event.payload.type === "over") {
        handler({ type: "over" });
      } else {
        handler({ type: "cancel" });
      }
    });
  }

  pickPublicationUploads() {
    return pickBrowserFiles(
      ".cbz,.zip,.cbr,.rar,.cb7,.7z,.cbt,.tar,.tgz,.tbz,.tbz2,.txz,.epub,.pdf",
      true,
    );
  }

  async uploadPublication(file: File, password?: string) {
    const uploadId = await invoke<string>("begin_file_upload", {
      fileName: file.name,
      expectedSize: file.size,
    });
    try {
      const chunkSize = 512 * 1024;
      for (let offset = 0; offset < file.size; offset += chunkSize) {
        const bytes = new Uint8Array(
          await file.slice(offset, offset + chunkSize).arrayBuffer(),
        );
        await invoke<number>("append_file_upload", {
          uploadId,
          chunk: bytesToBase64(bytes),
        });
      }
      return await invoke<LibraryItem>("finish_publication_upload", {
        uploadId,
        password: password ?? null,
      });
    } catch (error) {
      await invoke<boolean>("cancel_file_upload", { uploadId }).catch(
        () => false,
      );
      throw error;
    }
  }

  async pickPublications() {
    const selection = await open({
      title: tr("Add comics to Koma"),
      multiple: true,
      directory: false,
      pickerMode: "document",
      fileAccessMode: "copy",
      filters: [
        {
          name: tr("Comics and books"),
          extensions: [
            "cbz",
            "zip",
            "cbr",
            "rar",
            "cb7",
            "7z",
            "cbt",
            "tar",
            "tgz",
            "tbz",
            "tbz2",
            "txz",
            "epub",
            "pdf",
          ],
        },
      ],
    });
    if (selection === null) return [];
    return Array.isArray(selection) ? selection : [selection];
  }

  async pickRelinkSource() {
    const selection = await open({
      title: tr("Find the moved publication"),
      multiple: false,
      directory: false,
      pickerMode: "document",
      fileAccessMode: "copy",
      filters: [
        {
          name: tr("Comics and books"),
          extensions: [
            "cbz",
            "zip",
            "cbr",
            "rar",
            "cb7",
            "7z",
            "cbt",
            "tar",
            "tgz",
            "tbz",
            "tbz2",
            "txz",
            "epub",
            "pdf",
          ],
        },
      ],
    });
    return typeof selection === "string" ? selection : null;
  }

  async pickFolder() {
    const selection = await open({
      title: tr("Choose a library folder"),
      directory: true,
      multiple: false,
      fileAccessMode: "copy",
    });
    return typeof selection === "string" ? selection : null;
  }

  async pickConnectorPackage() {
    const selection = await open({
      title: tr("Import Koma connector"),
      directory: false,
      multiple: false,
      pickerMode: "document",
      fileAccessMode: "copy",
      filters: [
        {
          name: tr("Koma connector"),
          extensions: ["json"],
        },
      ],
    });
    return typeof selection === "string" ? selection : null;
  }

  async pickConnectorUpload() {
    const files = await pickBrowserFiles(
      ".koma-connector.json,.json,application/json",
      false,
    );
    return files[0] ?? null;
  }

  async inspectConnectorUpload(file: File) {
    return invoke<ConnectorPackagePreview>("inspect_connector_contents", {
      contents: await file.text(),
    });
  }

  pickBackupDestination() {
    return save({
      title: tr("Export Koma library backup"),
      defaultPath: "Koma Library.koma-backup.json",
      filters: [{ name: tr("Koma backup"), extensions: ["json"] }],
    });
  }

  async pickBackupSource() {
    const selection = await open({
      title: tr("Restore Koma library backup"),
      directory: false,
      multiple: false,
      pickerMode: "document",
      fileAccessMode: "copy",
      filters: [{ name: tr("Koma backup"), extensions: ["json"] }],
    });
    return typeof selection === "string" ? selection : null;
  }

  pickCbzDestination(suggestedName: string) {
    return save({
      title: tr("Save converted CBZ"),
      defaultPath: suggestedName,
      filters: [{ name: tr("Comic Book ZIP"), extensions: ["cbz"] }],
    });
  }

  addPublication(path: string, password?: string) {
    return invoke<LibraryItem>("add_publication", {
      path,
      password: password ?? null,
    });
  }

  relinkPublication(publicationId: string, path: string, password?: string) {
    return invoke<LibraryItem>("relink_publication", {
      publicationId,
      path,
      password: password ?? null,
    });
  }

  scanFolder(path: string) {
    return invoke<LibraryScanReport>("scan_folder", { path });
  }

  listLibraryFolders() {
    return invoke<LibraryFolder[]>("list_library_folders");
  }

  addLibraryFolder(path: string, scanIntervalMinutes: number) {
    return invoke<LibraryFolderScanResult>("add_library_folder", {
      path,
      scanIntervalMinutes,
    });
  }

  updateLibraryFolder(
    folderId: string,
    enabled: boolean,
    scanIntervalMinutes: number,
  ) {
    return invoke<LibraryFolder>("update_library_folder", {
      folderId,
      enabled,
      scanIntervalMinutes,
    });
  }

  removeLibraryFolder(folderId: string) {
    return invoke<boolean>("remove_library_folder", { folderId });
  }

  scanLibraryFolder(folderId: string) {
    return invoke<LibraryFolderScanResult>("scan_library_folder", { folderId });
  }

  listConnectors() {
    return invoke<ConnectorSummary[]>("list_connectors");
  }

  inspectConnectorPackage(path: string) {
    return invoke<ConnectorPackagePreview>("inspect_connector_package", {
      path,
    });
  }

  installConnectorPackage(installToken: string) {
    return invoke<ConnectorSummary>("install_connector_package", {
      installToken,
    });
  }

  removeConnector(connectorId: string) {
    return invoke<boolean>("remove_connector", { connectorId });
  }

  removeFromLibrary(publicationId: string) {
    return invoke<boolean>("remove_from_library", { publicationId });
  }

  setHidden(publicationId: string, hidden: boolean) {
    return invoke<boolean>("set_hidden", { publicationId, hidden });
  }

  setFavorite(publicationId: string, favorite: boolean) {
    return invoke<boolean>("set_favorite", { publicationId, favorite });
  }

  setReadingStatus(publicationId: string, completed: boolean) {
    return invoke<ReadingState>("set_reading_status", {
      publicationId,
      completed,
    });
  }

  openReader(publicationId: string, password?: string) {
    return invoke<ReaderOpenPayload>("open_reader", {
      publicationId,
      password: password ?? null,
    });
  }

  readPage(publicationId: string, pageIndex: number) {
    return invoke<PagePayload>("read_page", { publicationId, pageIndex });
  }

  saveProgress(
    publicationId: string,
    currentPage: number,
    settings?: ReaderSettings,
  ) {
    return invoke<ReadingState>("save_progress", {
      publicationId,
      currentPage,
      settings: settings ?? null,
    });
  }

  recordReadingTime(publicationId: string, elapsedSeconds: number) {
    return invoke<number>("record_reading_time", {
      publicationId,
      elapsedSeconds,
    });
  }

  trackingAccounts() {
    return invoke<TrackingAccount[]>("tracking_accounts");
  }

  async beginTrackingOAuth(provider: TrackingProvider) {
    const authorizationUrl = await invoke<string>("begin_tracking_oauth", {
      provider,
    });
    await openUrl(authorizationUrl);
  }

  takeTrackingAuth() {
    return invoke<TrackingAuthEvent | null>("take_tracking_auth");
  }

  onTrackingAuth(handler: (event: TrackingAuthEvent) => void) {
    return listen<TrackingAuthEvent>("koma://tracking-auth", ({ payload }) =>
      handler(payload),
    );
  }

  disconnectTracking(provider: TrackingProvider) {
    return invoke<void>("disconnect_tracking", { provider });
  }

  suggestTracking(provider: TrackingProvider, query: string) {
    return invoke<TrackingSuggestion>("suggest_tracking", { provider, query });
  }

  trackingMappings(publicationId: string) {
    return invoke<TrackingMapping[]>("tracking_mappings", { publicationId });
  }

  setTrackingMapping(mapping: TrackingMapping) {
    return invoke<void>("set_tracking_mapping", { mapping });
  }

  removeTrackingMapping(publicationId: string, provider: TrackingProvider) {
    return invoke<void>("remove_tracking_mapping", {
      publicationId,
      provider,
    });
  }

  trackingRemoteProgress(publicationId: string) {
    return invoke<TrackingRemoteProgress[]>("tracking_remote_progress", {
      publicationId,
    });
  }

  addBookmark(
    publicationId: string,
    pageIndex: number,
    label?: string,
    note?: string,
  ) {
    return invoke<Bookmark>("add_bookmark", {
      publicationId,
      pageIndex,
      label: label ?? null,
      note: note ?? null,
    });
  }

  updateBookmark(bookmarkId: string, label?: string, note?: string) {
    return invoke<Bookmark>("update_bookmark", {
      bookmarkId,
      label: label ?? null,
      note: note ?? null,
    });
  }

  removeBookmark(bookmarkId: string) {
    return invoke<boolean>("remove_bookmark", { bookmarkId });
  }

  previewLink(source: string) {
    return invoke<ImportPreview>("preview_link", { source });
  }

  async importLink(source: string, options: ImportOptions) {
    const jobId = uuid();
    try {
      return await invoke<LinkImportResult>("import_link", {
        source,
        options,
        jobId,
      });
    } finally {
      notifyImportFinished(jobId);
    }
  }

  readLinkOnline(source: string, options: ImportOptions) {
    return invoke<LibraryItem>("read_link_online", { source, options });
  }

  navigateOnlinePublication(
    publicationId: string,
    targetScope: "chapter" | "volume",
    targetId: number,
  ) {
    return invoke<OnlineNavigationResult>("navigate_online_publication", {
      publicationId,
      targetScope,
      targetId,
    });
  }

  async downloadOnlinePublication(
    publicationId: string,
    options?: ImportOptions,
  ) {
    const jobId = uuid();
    try {
      return await invoke<LinkImportResult>("download_online_publication", {
        publicationId,
        options: options ?? null,
        jobId,
      });
    } finally {
      notifyImportFinished(jobId);
    }
  }

  onImportEvent(handler: (event: ImportEvent, jobId: string) => void) {
    return listen<ImportEventPayload>("koma://import-event", (event) => {
      handler(event.payload.event, event.payload.jobId);
    });
  }

  cancelImport(jobId: string) {
    return invoke<boolean>("cancel_import", { jobId });
  }

  setDiscordPresence(enabled: boolean, details: string, activityState: string) {
    return invoke<boolean>("set_discord_presence", {
      enabled,
      details,
      activityState,
    });
  }

  exportLibraryBackup(destination: string) {
    return invoke<string>("export_library_backup", { destination });
  }

  restoreLibraryBackup(source: string) {
    return invoke<BackupRestoreReport>("restore_library_backup", { source });
  }

  inspectPublication(publicationId: string, password?: string) {
    return invoke<PublicationInspection>("inspect_library_publication", {
      publicationId,
      password: password ?? null,
    });
  }

  convertPublication(
    publicationId: string,
    destination: string,
    options: ConversionOptions,
    password?: string,
  ) {
    return invoke<PublicationOperationResult>("convert_library_publication", {
      publicationId,
      destination,
      password: password ?? null,
      options,
    });
  }

  repairPublication(
    publicationId: string,
    destination: string,
    password?: string,
  ) {
    return invoke<PublicationOperationResult>("repair_library_publication", {
      publicationId,
      destination,
      password: password ?? null,
    });
  }

  saveMetadata(
    publicationId: string,
    metadata: PublicationMetadata,
    writeToSource: boolean,
  ) {
    return invoke<MetadataSaveResult>("save_publication_metadata", {
      publicationId,
      metadata,
      writeToSource,
    });
  }

  revealItem(path: string) {
    return revealItemInDir(path);
  }

  publicationSourceUrl(path: string) {
    return convertFileSrc(path);
  }
}

function uuid(): string {
  return globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random()}`;
}

function notifyImportFinished(jobId: string) {
  window.dispatchEvent(
    new CustomEvent<string>("koma:import-finished", { detail: jobId }),
  );
}

class PreviewBackend implements KomaBackend {
  readonly kind = "preview" as const;
  private importHandlers = new Set<
    (event: ImportEvent, jobId: string) => void
  >();
  private cancelledImports = new Set<string>();
  private tracking = new Map<TrackingProvider, TrackingAccount>();
  private mappings: TrackingMapping[] = [];
  private trackingAuthHandlers = new Set<(event: TrackingAuthEvent) => void>();

  bootstrap() {
    return Promise.resolve(demoBootstrap());
  }

  listLibrary(includeHidden: boolean, search?: string) {
    const query = search?.trim().toLocaleLowerCase() ?? "";
    return Promise.resolve(
      readDemoItems().filter((item) => {
        if (!includeHidden && item.isHidden) return false;
        return (
          query.length === 0 ||
          item.title.toLocaleLowerCase().includes(query) ||
          item.series?.toLocaleLowerCase().includes(query) === true
        );
      }),
    );
  }

  takeOpenPaths() {
    return Promise.resolve([]);
  }

  onOpenPaths() {
    return Promise.resolve(() => undefined);
  }

  onLibraryChanged() {
    return Promise.resolve(() => undefined);
  }

  onFileDrop() {
    return Promise.resolve(() => undefined);
  }

  pickPublicationUploads() {
    return Promise.resolve([]);
  }

  uploadPublication(file: File) {
    return this.addPublication(`/Preview/${file.name}`);
  }

  pickPublications() {
    return Promise.resolve([]);
  }

  pickRelinkSource() {
    return Promise.resolve("/Preview/Relinked Publication.cbz");
  }

  pickFolder() {
    return Promise.resolve(null);
  }

  pickConnectorPackage() {
    return Promise.resolve(null);
  }

  pickConnectorUpload() {
    return Promise.resolve(null);
  }

  pickBackupDestination() {
    return Promise.resolve(null);
  }

  pickBackupSource() {
    return Promise.resolve(null);
  }

  pickCbzDestination(suggestedName: string) {
    return Promise.resolve(`/Preview/${suggestedName}`);
  }

  addPublication(path: string) {
    const items = readDemoItems();
    const template = items[0];
    if (template === undefined) throw new Error("Demo library is unavailable");
    const title =
      path
        .split(/[\\/]/)
        .pop()
        ?.replace(/\.[^.]+$/, "") ?? "Untitled";
    const item: LibraryItem = {
      ...template,
      id: uuid(),
      path,
      title,
      series: null,
      number: null,
      volume: null,
      currentPage: 0,
      progress: 0,
      isCompleted: false,
      isFavorite: false,
      addedAt: new Date().toISOString(),
      lastOpenedAt: null,
    };
    writeDemoItems([item, ...items]);
    return Promise.resolve(item);
  }

  relinkPublication(publicationId: string, path: string) {
    const item = this.item(publicationId);
    const updated = { ...item, path, isMissing: false };
    this.update(publicationId, () => updated);
    return Promise.resolve(updated);
  }

  async scanFolder(path: string) {
    const item = await this.addPublication(`${path}/Preview Collection.cbz`);
    return { imported: [item], skipped: [], failures: [], unchanged: 0 };
  }

  listLibraryFolders() {
    return Promise.resolve(
      JSON.parse(
        localStorage.getItem("koma.demo.folders") ?? "[]",
      ) as LibraryFolder[],
    );
  }

  async addLibraryFolder(path: string, scanIntervalMinutes: number) {
    const folders = await this.listLibraryFolders();
    const existing = folders.find((folder) => folder.path === path);
    const report = await this.scanFolder(path);
    const folder: LibraryFolder = {
      id: existing?.id ?? uuid(),
      path,
      enabled: true,
      scanIntervalMinutes,
      lastScannedAt: new Date().toISOString(),
      lastImportedCount: report.imported.length,
      lastFailureCount: report.failures.length,
      lastError: null,
    };
    const next = [
      folder,
      ...folders.filter((candidate) => candidate.id !== folder.id),
    ];
    localStorage.setItem("koma.demo.folders", JSON.stringify(next));
    return { folder, report };
  }

  async updateLibraryFolder(
    folderId: string,
    enabled: boolean,
    scanIntervalMinutes: number,
  ) {
    const folders = await this.listLibraryFolders();
    const folder = folders.find((candidate) => candidate.id === folderId);
    if (folder === undefined) throw new Error("Library folder was not found");
    const updated = { ...folder, enabled, scanIntervalMinutes };
    localStorage.setItem(
      "koma.demo.folders",
      JSON.stringify(
        folders.map((candidate) =>
          candidate.id === folderId ? updated : candidate,
        ),
      ),
    );
    return updated;
  }

  async removeLibraryFolder(folderId: string) {
    const folders = await this.listLibraryFolders();
    const next = folders.filter((folder) => folder.id !== folderId);
    localStorage.setItem("koma.demo.folders", JSON.stringify(next));
    return next.length !== folders.length;
  }

  async scanLibraryFolder(folderId: string) {
    const folders = await this.listLibraryFolders();
    const folder = folders.find((candidate) => candidate.id === folderId);
    if (folder === undefined) throw new Error("Library folder was not found");
    const report = await this.scanFolder(folder.path);
    const updated = {
      ...folder,
      lastScannedAt: new Date().toISOString(),
      lastImportedCount: report.imported.length,
      lastFailureCount: report.failures.length,
      lastError: null,
    };
    localStorage.setItem(
      "koma.demo.folders",
      JSON.stringify(
        folders.map((candidate) =>
          candidate.id === folderId ? updated : candidate,
        ),
      ),
    );
    return { folder: updated, report };
  }

  listConnectors() {
    return Promise.resolve([
      {
        id: "mangafire",
        name: "MangaFire",
        version: "mangafire-api-2026.07-chapter-series.2",
        description: null,
        kind: "bundled" as const,
        enabled: true,
        removable: false,
        schemaVersion: 0,
        runsCode: false,
        capabilities: ["chapter", "volume", "series"] as Array<
          "chapter" | "volume" | "series"
        >,
      },
    ]);
  }

  inspectConnectorPackage(): Promise<ConnectorPackagePreview> {
    return Promise.reject(
      commandError(
        "operation_failed",
        tr("Connector packages require the desktop app."),
      ),
    );
  }

  inspectConnectorUpload(): Promise<ConnectorPackagePreview> {
    return Promise.reject(
      commandError(
        "operation_failed",
        tr("Connector packages require the desktop app."),
      ),
    );
  }

  installConnectorPackage(): Promise<ConnectorSummary> {
    return Promise.reject(
      commandError(
        "operation_failed",
        tr("Connector packages require the desktop app."),
      ),
    );
  }

  removeConnector() {
    return Promise.resolve(false);
  }

  removeFromLibrary(publicationId: string) {
    const items = readDemoItems();
    const next = items.filter((item) => item.id !== publicationId);
    writeDemoItems(next);
    return Promise.resolve(next.length !== items.length);
  }

  setHidden(publicationId: string, hidden: boolean) {
    return Promise.resolve(
      this.update(publicationId, (item) => ({ ...item, isHidden: hidden })),
    );
  }

  setFavorite(publicationId: string, favorite: boolean) {
    return Promise.resolve(
      this.update(publicationId, (item) => ({
        ...item,
        isFavorite: favorite,
      })),
    );
  }

  setReadingStatus(publicationId: string, completed: boolean) {
    const item = this.item(publicationId);
    const currentPage = completed ? Math.max(0, item.pageCount - 1) : 0;
    const progress = completed && item.pageCount > 0 ? 1 : 0;
    const updatedAt = new Date().toISOString();
    this.update(publicationId, (candidate) => ({
      ...candidate,
      currentPage,
      progress,
      isCompleted: completed && item.pageCount > 0,
    }));
    return Promise.resolve({
      publicationId,
      currentPage,
      currentChapter: item.currentChapter,
      progress,
      completed: completed && item.pageCount > 0,
      totalReadingSeconds: item.totalReadingSeconds,
      settings: DEFAULT_READER_SETTINGS,
      updatedAt,
    });
  }

  openReader(publicationId: string) {
    const item = this.item(publicationId);
    return Promise.resolve(demoOpenPayload(item));
  }

  readPage(publicationId: string, pageIndex: number) {
    const item = this.item(publicationId);
    if (pageIndex < 0 || pageIndex >= item.pageCount) {
      throw commandError(
        "page_out_of_range",
        `Page ${pageIndex + 1} does not exist`,
      );
    }
    return Promise.resolve(demoPagePayload(item, pageIndex));
  }

  saveProgress(
    publicationId: string,
    currentPage: number,
    settings = DEFAULT_READER_SETTINGS,
  ) {
    const item = this.item(publicationId);
    const bounded = Math.max(0, Math.min(item.pageCount - 1, currentPage));
    const progress = item.pageCount <= 1 ? 1 : bounded / (item.pageCount - 1);
    const updatedAt = new Date().toISOString();
    this.update(publicationId, (candidate) => ({
      ...candidate,
      currentPage: bounded,
      progress,
      isCompleted: progress >= 1,
      lastOpenedAt: updatedAt,
    }));
    return Promise.resolve({
      publicationId,
      currentPage: bounded,
      currentChapter: item.currentChapter,
      progress,
      completed: progress >= 1,
      totalReadingSeconds: item.totalReadingSeconds,
      settings,
      updatedAt,
    });
  }

  recordReadingTime(publicationId: string, elapsedSeconds: number) {
    const item = this.item(publicationId);
    const totalReadingSeconds =
      item.totalReadingSeconds + Math.max(0, Math.min(120, elapsedSeconds));
    this.update(publicationId, (candidate) => ({
      ...candidate,
      totalReadingSeconds,
    }));
    return Promise.resolve(totalReadingSeconds);
  }

  trackingAccounts() {
    return Promise.resolve(
      (["aniList", "myAnimeList"] as TrackingProvider[]).map(
        (provider) =>
          this.tracking.get(provider) ?? {
            provider,
            connected: false,
            username: null,
            oauthConfigured: true,
          },
      ),
    );
  }

  beginTrackingOAuth(provider: TrackingProvider) {
    const account = {
      provider,
      connected: true,
      username: provider === "aniList" ? "KomaReader" : "koma_reader",
      oauthConfigured: true,
    };
    this.tracking.set(provider, account);
    for (const handler of this.trackingAuthHandlers) {
      handler({ success: true, message: "Account connected" });
    }
    return Promise.resolve();
  }

  takeTrackingAuth() {
    return Promise.resolve(null);
  }

  onTrackingAuth(handler: (event: TrackingAuthEvent) => void) {
    this.trackingAuthHandlers.add(handler);
    return Promise.resolve(() => this.trackingAuthHandlers.delete(handler));
  }

  disconnectTracking(provider: TrackingProvider) {
    this.tracking.delete(provider);
    this.mappings = this.mappings.filter(
      (mapping) => mapping.provider !== provider,
    );
    return Promise.resolve();
  }

  suggestTracking(provider: TrackingProvider, query: string) {
    return Promise.resolve({
      provider,
      automatic: true,
      candidates: [
        {
          id: provider === "aniList" ? 30013 : 13,
          title: query,
          alternateTitles: [],
          coverUrl: null,
          chapters: null,
          score: 1,
        },
      ],
    });
  }

  trackingMappings(publicationId: string) {
    return Promise.resolve(
      this.mappings.filter(
        (mapping) => mapping.publicationId === publicationId,
      ),
    );
  }

  setTrackingMapping(mapping: TrackingMapping) {
    this.mappings = this.mappings.filter(
      (candidate) =>
        candidate.publicationId !== mapping.publicationId ||
        candidate.provider !== mapping.provider,
    );
    this.mappings.push(mapping);
    return Promise.resolve();
  }

  removeTrackingMapping(publicationId: string, provider: TrackingProvider) {
    this.mappings = this.mappings.filter(
      (mapping) =>
        mapping.publicationId !== publicationId ||
        mapping.provider !== provider,
    );
    return Promise.resolve();
  }

  trackingRemoteProgress(publicationId: string) {
    return Promise.resolve(
      this.mappings
        .filter((mapping) => mapping.publicationId === publicationId)
        .map((mapping) => ({
          provider: mapping.provider,
          mediaId: mapping.mediaId,
          progress: mapping.lastSyncedChapter ?? 0,
          totalChapters: null,
          status: "reading",
          updatedAt: null,
        })),
    );
  }

  addBookmark(
    publicationId: string,
    pageIndex: number,
    label?: string,
    note?: string,
  ) {
    return Promise.resolve({
      id: uuid(),
      publicationId,
      pageIndex,
      label: label ?? null,
      note: note ?? null,
      createdAt: new Date().toISOString(),
    });
  }

  updateBookmark(bookmarkId: string, label?: string, note?: string) {
    return Promise.resolve({
      id: bookmarkId,
      publicationId: "",
      pageIndex: 0,
      label: label ?? null,
      note: note ?? null,
      createdAt: new Date().toISOString(),
    });
  }

  removeBookmark() {
    return Promise.resolve(true);
  }

  previewLink(source: string) {
    if (
      !/^https:\/\/mangafire\.to\/title\/[a-z0-9-]+(?:\/volume\/\d+)?(?:\?.*)?$/i.test(
        source.trim(),
      )
    ) {
      throw commandError(
        "unsupported_format",
        tr("Paste a MangaFire title or volume link."),
      );
    }
    return Promise.resolve({
      provider: "MangaFire",
      title: "Hatori to Furuta no Hinichijou Sahanji",
      sourceUrl: source.trim(),
      eligibilityUrl: source.trim().split("?")[0] ?? source.trim(),
      eligibilityStatus: 200,
      eligible: true,
      warning: demoBootstrap().importWarning,
      volumes: [
        {
          id: 339405,
          number: 1,
          name: null,
          language: "en",
          chapterCount: 15,
          pageCount: 397,
          selected: true,
        },
        {
          id: 342470,
          number: 1,
          name: null,
          language: "es-la",
          chapterCount: 15,
          pageCount: null,
          selected: false,
        },
        {
          id: 340352,
          number: 1,
          name: null,
          language: "pt-br",
          chapterCount: 15,
          pageCount: null,
          selected: false,
        },
      ],
      chapters: [
        {
          id: 1,
          number: 1,
          name: null,
          language: "en",
          pageCount: 59,
          selected: false,
        },
        {
          id: 17,
          number: 17,
          name: null,
          language: "en",
          pageCount: 19,
          selected: true,
        },
      ],
      selectedVolumeId: 339405,
      selectedChapterId: 17,
      estimatedPageCount: 397,
      seriesChapterCount: 17,
      seriesPageCount: 435,
      availableScopes: ["chapter", "volume", "series"] as Array<
        "chapter" | "volume" | "series"
      >,
    });
  }

  async importLink(source: string, options: ImportOptions) {
    const jobId = uuid();
    const preview = await this.previewLink(source);
    this.emit({ kind: "checking", url: preview.eligibilityUrl }, jobId);
    this.emit({ kind: "eligible", status: 200 }, jobId);
    const pageCount =
      options.scope === "chapter" ? 19 : options.scope === "series" ? 435 : 397;
    this.emit(
      {
        kind: "discovered",
        title: preview.title,
        volume:
          options.scope === "chapter"
            ? "Chapter 17"
            : options.scope === "series"
              ? "Series"
              : "Volume 1",
        pageCount,
      },
      jobId,
    );
    try {
      for (const completed of [
        Math.min(pageCount, Math.ceil(pageCount * 0.2)),
        Math.min(pageCount, Math.ceil(pageCount * 0.5)),
        pageCount,
      ]) {
        await new Promise((resolve) => window.setTimeout(resolve, 80));
        if (this.cancelledImports.delete(jobId)) {
          throw new Error(tr("Download cancelled"));
        }
        this.emit({ kind: "downloading", completed, total: pageCount }, jobId);
      }
      const qualifier =
        options.scope === "chapter"
          ? "Ch. 17"
          : options.scope === "series"
            ? "Complete"
            : "Vol. 1";
      const outputPath = `${options.destinationDirectory}/${preview.title} — ${qualifier} [en].cbz`;
      this.emit({ kind: "packaging", outputPath }, jobId);
      const item = await this.addPublication(outputPath);
      const receipt = {
        id: uuid(),
        provider: "MangaFire",
        sourceUrl: source,
        eligibilityUrl: preview.eligibilityUrl,
        eligibilityStatus: 200,
        checkedAt: new Date().toISOString(),
        pageCount,
        outputPath,
        outputHash: "preview-only",
        adapterVersion: "browser-preview",
      };
      this.emit({ kind: "completed", receipt }, jobId);
      return { receipt, item };
    } finally {
      this.cancelledImports.delete(jobId);
      notifyImportFinished(jobId);
    }
  }

  async readLinkOnline(source: string, options: ImportOptions) {
    const result = await this.importLink(source, options);
    const item: LibraryItem = {
      ...result.item,
      format: "online",
      path: `koma-online://${result.item.id}`,
    };
    writeDemoItems(
      readDemoItems().map((candidate) =>
        candidate.id === item.id ? item : candidate,
      ),
    );
    return item;
  }

  async navigateOnlinePublication(
    publicationId: string,
    targetScope: "chapter" | "volume",
    targetId: number,
  ): Promise<OnlineNavigationResult> {
    const item = this.item(publicationId);
    const reader = await this.openReader(publicationId);
    const source = reader.onlineSource;
    if (source !== null && source !== undefined) {
      const catalog =
        targetScope === "chapter"
          ? source.chapterCatalog
          : source.volumeCatalog;
      const target = catalog.find((section) => section.id === targetId);
      const current = source.chapters[0];
      if (target !== undefined && current !== undefined) {
        source.chapterId =
          targetScope === "chapter" ? targetId : source.chapterId;
        source.volumeId = targetScope === "volume" ? targetId : source.volumeId;
        source.chapters[0] = {
          ...current,
          id:
            targetScope === "chapter" ? String(targetId) : `volume:${targetId}`,
          number: target.number,
          title: target.title,
          volume: targetScope === "volume" ? target.number : null,
        };
      }
    }
    return { item, reader };
  }

  async downloadOnlinePublication(
    publicationId: string,
    options?: ImportOptions,
  ) {
    const current = this.item(publicationId);
    const result = await this.importLink(
      "https://mangafire.to/title/70ox7-hatori-to-furuta-no-hinichijou-sahanji",
      options ?? {
        destinationDirectory: "/Downloads/Koma",
        volumeId: null,
        chapterId: null,
        selectedChapterIds: [],
        scope: "series",
        preferredLanguage: "en",
        overwriteExisting: false,
        downloadConcurrency: 6,
      },
    );
    const item = { ...result.item, id: current.id };
    writeDemoItems(
      readDemoItems()
        .filter((candidate) => candidate.id !== result.item.id)
        .map((candidate) =>
          candidate.id === publicationId ? item : candidate,
        ),
    );
    return { ...result, item };
  }

  onImportEvent(handler: (event: ImportEvent, jobId: string) => void) {
    this.importHandlers.add(handler);
    return Promise.resolve(() => {
      this.importHandlers.delete(handler);
    });
  }

  cancelImport(jobId: string) {
    this.cancelledImports.add(jobId);
    return Promise.resolve(true);
  }

  setDiscordPresence() {
    return Promise.resolve(false);
  }

  exportLibraryBackup() {
    return Promise.resolve("Koma Library.koma-backup.json");
  }

  restoreLibraryBackup() {
    return Promise.resolve({
      publications: 0,
      readingStates: 0,
      bookmarks: 0,
      importReceipts: 0,
      metadataOverrides: 0,
      onlineSources: 0,
      missingSources: 0,
    });
  }

  inspectPublication(publicationId: string) {
    const item = this.item(publicationId);
    return Promise.resolve({
      path: item.path,
      format: item.format,
      pageCount: item.pageCount,
      validatedPages: item.format === "pdf" ? 0 : item.pageCount,
      sourceBytes: item.pageCount * 860_000,
      duplicateGroups: [],
      issues: [
        {
          severity: "information" as const,
          code: "metadataIncomplete" as const,
          pageIndex: null,
          message: "The preview publication has only its essential metadata.",
        },
      ],
      metadata: demoOpenPayload(item).manifest.metadata,
    });
  }

  async convertPublication(
    publicationId: string,
    destination: string,
    options: ConversionOptions,
  ) {
    const source = this.item(publicationId);
    await new Promise((resolve) => window.setTimeout(resolve, 240));
    const item = await this.addPublication(destination);
    return {
      report: {
        outputPath: destination,
        sourceFormat: source.format,
        pageCount: source.pageCount,
        skippedPages: [],
        sourceBytes: source.pageCount * 860_000,
        outputBytes:
          source.pageCount *
          (options.imageFormat === "jpeg" ? 420_000 : 780_000),
        outputHash: "preview-only",
        backupPath: null,
      },
      item,
    };
  }

  repairPublication(publicationId: string, destination: string) {
    return this.convertPublication(publicationId, destination, {
      imageFormat: "original",
      jpegQuality: 90,
      maxDimension: null,
      skipUnreadablePages: true,
    });
  }

  saveMetadata(publicationId: string, metadata: PublicationMetadata) {
    const item = this.item(publicationId);
    const updated = {
      ...item,
      title: metadata.title,
      series: metadata.series,
      number: metadata.number,
      volume: metadata.volume,
    };
    this.update(publicationId, () => updated);
    return Promise.resolve({ item: updated, backupPath: null });
  }

  revealItem() {
    return Promise.resolve();
  }

  publicationSourceUrl(path: string) {
    return path;
  }

  private emit(event: ImportEvent, jobId: string) {
    for (const handler of this.importHandlers) handler(event, jobId);
  }

  private item(publicationId: string) {
    const item = readDemoItems().find(
      (candidate) => candidate.id === publicationId,
    );
    if (item === undefined)
      throw new Error("Publication is not in the demo library");
    return item;
  }

  private update(
    publicationId: string,
    update: (item: LibraryItem) => LibraryItem,
  ) {
    let found = false;
    const items = readDemoItems().map((item) => {
      if (item.id !== publicationId) return item;
      found = true;
      return update(item);
    });
    writeDemoItems(items);
    return found;
  }
}

function commandError(code: string, message: string): Error & CommandError {
  return Object.assign(new Error(message), {
    code,
    recoverable: true,
  });
}

export function isNativeKoma(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

export const backend: KomaBackend = isNativeKoma()
  ? new NativeBackend()
  : new PreviewBackend();

const ERROR_TRANSLATION_KEYS: Record<string, string> = {
  import_denied: "This import is not permitted.",
  provider_unavailable: "The import provider is unavailable.",
  provider_changed: "The source returned an unexpected response.",
  password_required: "This publication requires a password.",
  missing_source: "The source file could not be found.",
  unsupported_format: "This file format is not supported.",
  empty_publication: "This publication contains no readable pages.",
  page_out_of_range: "This page is not available.",
  unsafe_publication: "This publication did not pass its safety checks.",
  cancelled: "The operation was cancelled.",
  tracking_failed: "Reading tracking could not be completed.",
  operation_failed: "The operation could not be completed.",
};

function rawErrorMessage(error: unknown): string {
  if (
    typeof error === "object" &&
    error !== null &&
    "message" in error &&
    typeof error.message === "string"
  ) {
    return error.message;
  }
  return error instanceof Error ? error.message : String(error);
}

export function localizeMessage(message: string, fallbackKey: string): string {
  const translated = tr(message);
  if (locale().toLocaleLowerCase().startsWith("en") || translated !== message) {
    return translated;
  }
  return tr(fallbackKey);
}

export function errorMessage(error: unknown): string {
  const raw = rawErrorMessage(error);
  const key = errorCode(error);
  if (key !== null && ERROR_TRANSLATION_KEYS[key] !== undefined) {
    return locale().toLocaleLowerCase().startsWith("en")
      ? raw
      : tr(ERROR_TRANSLATION_KEYS[key]);
  }
  return localizeMessage(raw, "The operation could not be completed.");
}

export function errorCode(error: unknown): string | null {
  if (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    typeof error.code === "string"
  ) {
    return error.code;
  }
  return null;
}

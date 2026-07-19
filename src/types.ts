export type PublicationFormat =
  | "cbz"
  | "cbr"
  | "cb7"
  | "cbt"
  | "folder"
  | "pdf"
  | "fixedLayoutEpub";

export type ReadingDirection =
  | "automatic"
  | "leftToRight"
  | "rightToLeft"
  | "vertical";

export type ReaderMode =
  | "singlePage"
  | "spreads"
  | "continuous"
  | "webtoon"
  | "guided"
  | "presentation";

export type FitMode = "smart" | "page" | "width" | "height" | "original";
export type WidePagePolicy = "keep" | "split" | "rotate";

export interface ReaderSettings {
  mode: ReaderMode;
  direction: ReadingDirection;
  fit: FitMode;
  widePagePolicy: WidePagePolicy;
  cropMargins: boolean;
  gapPx: number;
  spreadGapEnabled: boolean;
  brightness: number;
  contrast: number;
  saturation: number;
  gamma: number;
  grayscale: boolean;
  invert: boolean;
  sharpen: boolean;
  keepAwake: boolean;
  showPageNumber: boolean;
}

export interface LibraryItem {
  id: string;
  path: string;
  format: PublicationFormat;
  title: string;
  series: string | null;
  number: string | null;
  volume: number | null;
  pageCount: number;
  currentPage: number;
  currentChapter: number | null;
  progress: number;
  totalReadingSeconds: number;
  isCompleted: boolean;
  isHidden: boolean;
  isMissing: boolean;
  isFavorite: boolean;
  coverDataUrl: string | null;
  addedAt: string;
  lastOpenedAt: string | null;
}

export interface PublicationMetadata {
  title: string;
  series: string | null;
  number: string | null;
  volume: number | null;
  summary: string | null;
  writer: string | null;
  penciller: string | null;
  publisher: string | null;
  language: string | null;
  genres: string[];
  tags: string[];
  web: string | null;
  direction: ReadingDirection;
}

export interface PageDescriptor {
  index: number;
  label: string;
  sourceName: string;
  mimeType: string;
  byteSize: number;
  width: number | null;
  height: number | null;
  isCover: boolean;
}

export interface PublicationManifest {
  id: string;
  path: string;
  format: PublicationFormat;
  metadata: PublicationMetadata;
  pages: PageDescriptor[];
  chapters: ChapterRange[];
  fingerprint: string;
  modifiedAt: string | null;
}

export interface ChapterRange {
  id: string | null;
  number: number;
  title: string | null;
  startPageIndex: number;
  endPageIndex: number;
}

export interface ReadingState {
  publicationId: string;
  currentPage: number;
  currentChapter: number | null;
  progress: number;
  completed: boolean;
  totalReadingSeconds: number;
  settings: ReaderSettings;
  updatedAt: string;
}

export interface Bookmark {
  id: string;
  publicationId: string;
  pageIndex: number;
  label: string | null;
  note: string | null;
  createdAt: string;
}

export interface ReaderOpenPayload {
  libraryId: string;
  manifest: PublicationManifest;
  readingState: ReadingState | null;
  bookmarks: Bookmark[];
}

export interface PagePayload {
  index: number;
  mimeType: string;
  dataUrl: string;
}

export interface BootstrapPayload {
  items: LibraryItem[];
  defaultImportDirectory: string;
  defaultReaderSettings: ReaderSettings;
  importWarning: string;
  appVersion: string;
  platform: string;
  supportedFormats: string[];
}

export interface LibraryScanFailure {
  path: string;
  reason: string;
}

export interface LibraryScanReport {
  imported: LibraryItem[];
  skipped: string[];
  failures: LibraryScanFailure[];
  unchanged: number;
}

export interface LibraryFolder {
  id: string;
  path: string;
  enabled: boolean;
  scanIntervalMinutes: number;
  lastScannedAt: string | null;
  lastImportedCount: number;
  lastFailureCount: number;
  lastError: string | null;
}

export interface LibraryFolderScanResult {
  folder: LibraryFolder;
  report: LibraryScanReport;
}

export type ConnectorKind = "bundled" | "declarative";

export interface ConnectorSummary {
  id: string;
  name: string;
  version: string;
  description: string | null;
  kind: ConnectorKind;
  enabled: boolean;
  removable: boolean;
  schemaVersion: number;
  runsCode: boolean;
  capabilities: Array<"chapter" | "volume" | "series">;
}

export interface ConnectorPackagePreview {
  installToken: string;
  connector: ConnectorSummary;
  allowedRequestHosts: string[];
  allowedPageHosts: string[];
  allowLocalNetwork: boolean;
}

export type OutputImageFormat = "original" | "jpeg" | "png" | "webp";

export interface ConversionOptions {
  imageFormat: OutputImageFormat;
  jpegQuality: number;
  maxDimension: number | null;
  skipUnreadablePages: boolean;
}

export interface SkippedPage {
  index: number;
  sourceName: string;
  reason: string;
}

export interface ConversionReport {
  outputPath: string;
  sourceFormat: PublicationFormat;
  pageCount: number;
  skippedPages: SkippedPage[];
  sourceBytes: number;
  outputBytes: number;
  outputHash: string;
  backupPath: string | null;
}

export interface PublicationOperationResult {
  report: ConversionReport;
  item: LibraryItem;
}

export type InspectionSeverity = "information" | "warning" | "error";
export type InspectionIssueCode =
  | "metadataIncomplete"
  | "duplicateContent"
  | "extensionMismatch"
  | "unreadablePage"
  | "veryLargePage"
  | "widePages"
  | "pdfManifestOnly";

export interface InspectionIssue {
  severity: InspectionSeverity;
  code: InspectionIssueCode;
  pageIndex: number | null;
  message: string;
}

export interface PublicationInspection {
  path: string;
  format: PublicationFormat;
  pageCount: number;
  validatedPages: number;
  sourceBytes: number;
  duplicateGroups: number[][];
  issues: InspectionIssue[];
  metadata: PublicationMetadata;
}

export interface MetadataSaveResult {
  item: LibraryItem;
  backupPath: string | null;
}

export interface BackupRestoreReport {
  publications: number;
  readingStates: number;
  bookmarks: number;
  importReceipts: number;
  metadataOverrides: number;
  missingSources: number;
}

export interface ImportOptions {
  destinationDirectory: string;
  volumeId: number | null;
  chapterId: number | null;
  selectedChapterIds: number[];
  scope: "chapter" | "volume" | "series";
  preferredLanguage: string | null;
  overwriteExisting: boolean;
  downloadConcurrency: number;
}

export type TrackingProvider = "aniList" | "myAnimeList";

export interface TrackingAccount {
  provider: TrackingProvider;
  connected: boolean;
  username: string | null;
  oauthConfigured: boolean;
}

export interface TrackingAuthEvent {
  success: boolean;
  message: string;
}

export interface TrackingCandidate {
  id: number;
  title: string;
  alternateTitles: string[];
  coverUrl: string | null;
  chapters: number | null;
  score: number;
}

export interface TrackingSuggestion {
  provider: TrackingProvider;
  automatic: boolean;
  candidates: TrackingCandidate[];
}

export interface TrackingMapping {
  publicationId: string;
  provider: TrackingProvider;
  mediaId: number;
  mediaTitle: string;
}

export interface ImportVolume {
  id: number;
  number: number;
  name: string | null;
  language: string;
  chapterCount: number | null;
  pageCount: number | null;
  selected: boolean;
}

export interface ImportChapter {
  id: number;
  number: number;
  name: string | null;
  language: string;
  pageCount: number | null;
  selected: boolean;
}

export interface ImportPreview {
  provider: string;
  title: string;
  sourceUrl: string;
  eligibilityUrl: string;
  eligibilityStatus: number;
  eligible: boolean;
  warning: string;
  volumes: ImportVolume[];
  chapters: ImportChapter[];
  selectedVolumeId: number | null;
  selectedChapterId: number | null;
  estimatedPageCount: number | null;
  seriesChapterCount: number | null;
  seriesPageCount: number | null;
  availableScopes: Array<"chapter" | "volume" | "series">;
}

export interface ImportReceipt {
  id: string;
  provider: string;
  sourceUrl: string;
  eligibilityUrl: string;
  eligibilityStatus: number;
  checkedAt: string;
  pageCount: number;
  outputPath: string;
  outputHash: string;
  adapterVersion: string;
}

export interface LinkImportResult {
  receipt: ImportReceipt;
  item: LibraryItem;
}

export type ImportEvent =
  | { kind: "checking"; url: string }
  | { kind: "eligible"; status: number }
  | { kind: "discovered"; title: string; volume: string; pageCount: number }
  | { kind: "downloading"; completed: number; total: number }
  | { kind: "recovering"; failedPages: number; strategy: string }
  | { kind: "packaging"; outputPath: string }
  | { kind: "completed"; receipt: ImportReceipt };

export interface CommandError {
  code: string;
  message: string;
  recoverable: boolean;
}

export type LibraryRoute =
  | "home"
  | "library"
  | "continue"
  | "favorites"
  | "hidden"
  | "settings";

export type LibraryViewMode = "grid" | "list";
export type LibrarySortMode =
  | "recent"
  | "title"
  | "series"
  | "added"
  | "progress";
export type ThemeMode = "system" | "light" | "dark";
export type LanguageMode = string;
export type MotionMode = "system" | "on" | "off";

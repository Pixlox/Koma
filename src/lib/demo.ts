import type {
  Bookmark,
  BootstrapPayload,
  LibraryItem,
  PageDescriptor,
  PublicationManifest,
  ReaderOpenPayload,
  ReaderSettings,
  ReadingState,
} from "../types";

export const DEFAULT_READER_SETTINGS: ReaderSettings = {
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

const DEMO_STORAGE_KEY = "koma.demo.library.v1";

function svgData(source: string): string {
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(source)}`;
}

function cover(
  title: string,
  kicker: string,
  background: string,
  ink: string,
  mark: string,
  pattern: "moon" | "train" | "stairs" | "window" | "wave" | "orbit",
): string {
  const art: Record<typeof pattern, string> = {
    moon: `<circle cx="210" cy="170" r="74" fill="${mark}"/><path d="M20 330 Q150 250 280 330V430H20Z" fill="${ink}" opacity=".86"/>`,
    train: `<rect x="38" y="155" width="224" height="122" rx="8" fill="${ink}"/><rect x="58" y="177" width="49" height="45" fill="${background}"/><rect x="126" y="177" width="49" height="45" fill="${background}"/><rect x="194" y="177" width="49" height="45" fill="${background}"/><circle cx="89" cy="286" r="18" fill="${mark}"/><circle cx="211" cy="286" r="18" fill="${mark}"/>`,
    stairs: `<path d="M38 305h50v-48h50v-48h50v-48h74v160H38Z" fill="${ink}"/><circle cx="222" cy="118" r="30" fill="${mark}"/>`,
    window: `<rect x="54" y="112" width="192" height="196" fill="${ink}"/><path d="M150 112v196M54 210h192" stroke="${background}" stroke-width="9"/><circle cx="196" cy="160" r="29" fill="${mark}"/>`,
    wave: `<path d="M18 212c47-74 87 74 134 0s87 74 130 0v164H18Z" fill="${ink}"/><path d="M18 178c47-74 87 74 134 0s87 74 130 0" fill="none" stroke="${mark}" stroke-width="18"/>`,
    orbit: `<circle cx="150" cy="214" r="54" fill="${ink}"/><ellipse cx="150" cy="214" rx="123" ry="62" fill="none" stroke="${mark}" stroke-width="12" transform="rotate(-18 150 214)"/><circle cx="264" cy="177" r="15" fill="${mark}"/>`,
  };
  return svgData(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 300 450">
    <rect width="300" height="450" fill="${background}"/>
    <rect x="18" y="18" width="264" height="414" fill="none" stroke="${ink}" stroke-width="2" opacity=".26"/>
    ${art[pattern]}
    <text x="30" y="58" fill="${ink}" font-family="system-ui,sans-serif" font-size="13" font-weight="650" letter-spacing="2.2">${kicker}</text>
    <text x="30" y="365" fill="${ink}" font-family="system-ui,sans-serif" font-size="29" font-weight="720">${title
      .split(" ")
      .slice(0, 3)
      .join(" ")}</text>
    <text x="30" y="399" fill="${ink}" font-family="system-ui,sans-serif" font-size="29" font-weight="720">${title
      .split(" ")
      .slice(3)
      .join(" ")}</text>
    <circle cx="260" cy="407" r="7" fill="${mark}"/>
  </svg>`);
}

const seeds = [
  {
    id: "018f1000-0000-7000-8000-000000000001",
    title: "After the Last Train",
    series: "After the Last Train",
    format: "cbz" as const,
    pages: 42,
    current: 17,
    background: "#d8d2c8",
    ink: "#2e2b28",
    mark: "#c9543b",
    pattern: "train" as const,
    kicker: "NIGHT LINE · 01",
  },
  {
    id: "018f1000-0000-7000-8000-000000000002",
    title: "Small Gods of Tuesday",
    series: "Small Gods",
    format: "cbr" as const,
    pages: 68,
    current: 0,
    background: "#b9c6bd",
    ink: "#20312b",
    mark: "#d36748",
    pattern: "stairs" as const,
    kicker: "VOLUME TWO",
  },
  {
    id: "018f1000-0000-7000-8000-000000000003",
    title: "The Salt Archive",
    series: null,
    format: "cb7" as const,
    pages: 126,
    current: 91,
    background: "#c8d1d0",
    ink: "#243033",
    mark: "#b54b39",
    pattern: "wave" as const,
    kicker: "FIELD NOTES",
  },
  {
    id: "018f1000-0000-7000-8000-000000000004",
    title: "Moon Over Nakano",
    series: "Moon Over Nakano",
    format: "fixedLayoutEpub" as const,
    pages: 54,
    current: 53,
    background: "#25282d",
    ink: "#eee9df",
    mark: "#d15b43",
    pattern: "moon" as const,
    kicker: "COMPLETE EDITION",
  },
  {
    id: "018f1000-0000-7000-8000-000000000005",
    title: "A Room Facing East",
    series: null,
    format: "folder" as const,
    pages: 31,
    current: 4,
    background: "#e3d7c1",
    ink: "#3c342b",
    mark: "#bb4c36",
    pattern: "window" as const,
    kicker: "QUIET STORIES",
  },
  {
    id: "018f1000-0000-7000-8000-000000000006",
    title: "Orbital Weather",
    series: "Orbital Weather",
    format: "cbz" as const,
    pages: 88,
    current: 0,
    background: "#c8c2d0",
    ink: "#2d2934",
    mark: "#c64f38",
    pattern: "orbit" as const,
    kicker: "ISSUE 04",
  },
];

function seededItems(): LibraryItem[] {
  const now = Date.now();
  return seeds.map((seed, index) => {
    const progress =
      seed.pages <= 1 ? 0 : Math.min(1, seed.current / (seed.pages - 1));
    return {
      id: seed.id,
      path: `/Koma Demo/${seed.title}.${seed.format === "folder" ? "pages" : seed.format}`,
      format: seed.format,
      title: seed.title,
      series: seed.series,
      number: index === 1 ? "2" : null,
      volume: index === 1 ? 2 : null,
      pageCount: seed.pages,
      currentPage: seed.current,
      progress,
      isCompleted: progress >= 1,
      isHidden: false,
      isMissing: false,
      isFavorite: index === 0 || index === 3,
      coverDataUrl: cover(
        seed.title,
        seed.kicker,
        seed.background,
        seed.ink,
        seed.mark,
        seed.pattern,
      ),
      addedAt: new Date(now - index * 86_400_000).toISOString(),
      lastOpenedAt:
        seed.current > 0
          ? new Date(now - index * 3_600_000).toISOString()
          : null,
    };
  });
}

export function readDemoItems(): LibraryItem[] {
  try {
    const saved = localStorage.getItem(DEMO_STORAGE_KEY);
    if (saved !== null) {
      const parsed = JSON.parse(saved) as LibraryItem[];
      if (Array.isArray(parsed) && parsed.length > 0) {
        return parsed;
      }
    }
  } catch {
    // A private browsing mode may reject localStorage. The demo still works.
  }
  return seededItems();
}

export function writeDemoItems(items: LibraryItem[]): void {
  try {
    localStorage.setItem(DEMO_STORAGE_KEY, JSON.stringify(items));
  } catch {
    // Persistence is a convenience in the browser preview, not a requirement.
  }
}

function demoPage(title: string, page: number, total: number): string {
  const variants = [
    { paper: "#f0ede6", ink: "#282725", quiet: "#c5c1b8", mark: "#c9543b" },
    { paper: "#ebe8df", ink: "#242628", quiet: "#bbc3c2", mark: "#b94b38" },
    { paper: "#f3eee5", ink: "#302b28", quiet: "#c9bdb2", mark: "#ce5a40" },
  ];
  const palette = variants[page % variants.length] ?? variants[0]!;
  const panelShift = (page * 37) % 110;
  return svgData(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1000 1500">
    <rect width="1000" height="1500" fill="${palette.paper}"/>
    <text x="72" y="84" fill="${palette.ink}" font-family="system-ui,sans-serif" font-size="24" letter-spacing="4">${title.toUpperCase()}</text>
    <g stroke="${palette.ink}" stroke-width="9" fill="none">
      <rect x="70" y="125" width="860" height="410"/>
      <rect x="70" y="565" width="410" height="370"/>
      <rect x="520" y="565" width="410" height="370"/>
      <rect x="70" y="965" width="860" height="420"/>
    </g>
    <g fill="${palette.quiet}">
      <circle cx="${235 + panelShift}" cy="315" r="118"/>
      <path d="M560 500V250h330v250z"/>
      <path d="M85 920L465 590v330z"/>
      <circle cx="725" cy="750" r="132"/>
      <path d="M95 1368l240-300 180 180 150-145 235 265z"/>
    </g>
    <g fill="${palette.ink}">
      <circle cx="${225 + panelShift}" cy="298" r="52"/>
      <path d="M${170 + panelShift} 460q55-165 110 0z"/>
      <circle cx="700" cy="730" r="48"/>
      <path d="M640 900q60-160 120 0z"/>
    </g>
    <g fill="${palette.paper}" stroke="${palette.ink}" stroke-width="6">
      <ellipse cx="440" cy="235" rx="155" ry="86"/>
      <ellipse cx="290" cy="690" rx="128" ry="72"/>
      <ellipse cx="765" cy="1100" rx="145" ry="78"/>
    </g>
    <g fill="${palette.ink}" font-family="system-ui,sans-serif" font-size="22" text-anchor="middle">
      <text x="440" y="228">Somewhere, the city</text><text x="440" y="260">kept one light on.</text>
      <text x="290" y="684">We noticed.</text><text x="290" y="716">That was enough.</text>
      <text x="765" y="1094">Tomorrow can wait</text><text x="765" y="1126">until morning.</text>
    </g>
    <circle cx="899" cy="1338" r="13" fill="${palette.mark}"/>
    <text x="500" y="1452" text-anchor="middle" fill="${palette.ink}" font-family="ui-monospace,monospace" font-size="20">${page + 1} / ${total}</text>
  </svg>`);
}

function descriptors(count: number): PageDescriptor[] {
  return Array.from({ length: count }, (_, index) => ({
    index,
    label: String(index + 1),
    sourceName: `${String(index + 1).padStart(3, "0")}.svg`,
    mimeType: "image/svg+xml",
    byteSize: 8_000,
    width: 1000,
    height: 1500,
    isCover: index === 0,
  }));
}

export function demoOpenPayload(item: LibraryItem): ReaderOpenPayload {
  const settings = {
    ...DEFAULT_READER_SETTINGS,
    direction: item.title === "Moon Over Nakano" ? ("rightToLeft" as const) : ("automatic" as const),
  };
  const readingState: ReadingState = {
    publicationId: item.id,
    currentPage: item.currentPage,
    progress: item.progress,
    completed: item.isCompleted,
    settings,
    updatedAt: item.lastOpenedAt ?? item.addedAt,
  };
  const manifest: PublicationManifest = {
    id: item.id,
    path: item.path,
    format: item.format,
    metadata: {
      title: item.title,
      series: item.series,
      number: item.number,
      volume: item.volume,
      summary:
        "On the last train home, two strangers notice that the empty stations have begun leaving messages for them.",
      writer: "M. Arai",
      penciller: null,
      publisher: null,
      language: "en",
      genres: ["Drama", "Slice of Life"],
      tags: ["Urban", "Night"],
      web: null,
      direction: settings.direction,
    },
    pages: descriptors(item.pageCount),
    fingerprint: `demo-${item.id}`,
    modifiedAt: item.addedAt,
  };
  const bookmarks: Bookmark[] =
    item.currentPage > 8
      ? [
          {
            id: `${item.id.slice(0, -1)}b`,
            publicationId: item.id,
            pageIndex: 8,
            label: "The platform",
            note: null,
            createdAt: item.addedAt,
          },
        ]
      : [];
  return {
    libraryId: item.id,
    manifest,
    readingState,
    bookmarks,
  };
}

export function demoPagePayload(item: LibraryItem, pageIndex: number) {
  return {
    index: pageIndex,
    mimeType: "image/svg+xml",
    dataUrl: demoPage(item.title, pageIndex, item.pageCount),
  };
}

export function demoBootstrap(): BootstrapPayload {
  return {
    items: readDemoItems(),
    defaultImportDirectory: "~/Downloads/Koma",
    defaultReaderSettings: DEFAULT_READER_SETTINGS,
    importWarning:
      "Only import properly released works that you own or have permission to download.",
    appVersion: "0.1.0-preview",
    platform: "browser",
    supportedFormats: [
      "CBZ/ZIP",
      "CBR/RAR",
      "CB7/7z",
      "CBT/TAR",
      "Folder",
      "EPUB",
      "PDF",
    ],
  };
}

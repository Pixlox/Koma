import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { useVirtualizer } from "@tanstack/react-virtual";
import {
  Archive,
  ArrowDownAZ,
  BookOpen,
  Check,
  ChevronDown,
  ChevronRight,
  CircleCheckBig,
  Clock3,
  Eye,
  EyeOff,
  ExternalLink,
  FileArchive,
  FolderPlus,
  FolderOpen,
  Grid2X2,
  Heart,
  Home,
  Import,
  LibraryBig,
  Link2,
  List,
  Languages,
  Menu,
  MoreHorizontal,
  Plus,
  Search,
  Settings,
  ShieldCheck,
  Sparkles,
  RefreshCw,
  Trash2,
  Wrench,
} from "lucide-react";
import {
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";

import {
  listAvailableLanguages,
  locale,
  tr,
} from "../i18n";
import { backend, errorMessage, localizeMessage } from "../lib/backend";
import {
  checkForUpdate,
  restartAfterUpdate,
  type AvailableUpdate,
} from "../lib/updater";
import { useKomaStore } from "../store/koma";
import type {
  LibraryFolder,
  LibraryItem,
  LibraryRoute,
  LibrarySortMode,
  LibraryViewMode,
  PublicationFormat,
  MotionMode,
  TrackingAccount,
  TrackingAuthEvent,
  TrackingCandidate,
  TrackingMapping,
  TrackingProvider,
  TrackingRemoteProgress,
  ThemeMode,
} from "../types";

const ROUTES: Array<{
  id: LibraryRoute;
  label: string;
  icon: typeof Home;
}> = [
  { id: "home", label: "Home", icon: Home },
  { id: "library", label: "Library", icon: LibraryBig },
  { id: "continue", label: "Continue", icon: Clock3 },
  { id: "favorites", label: "Favorites", icon: Heart },
  { id: "hidden", label: "Hidden", icon: EyeOff },
];

const FORMAT_LABELS: Record<PublicationFormat, string> = {
  cbz: "CBZ",
  cbr: "CBR",
  cb7: "CB7",
  cbt: "CBT",
  folder: "FOLDER",
  pdf: "PDF",
  fixedLayoutEpub: "EPUB",
};

function routeTitle(route: LibraryRoute): string {
  return tr(ROUTES.find((item) => item.id === route)?.label ?? "Settings");
}

function formatProgress(progress: number): string {
  return `${Math.round(Math.max(0, Math.min(1, progress)) * 100)}%`;
}

function formatReadingTime(seconds: number): string {
  if (seconds < 60) return tr("Less than a minute");
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (hours === 0) return tr("{{count}} min", { count: minutes });
  if (minutes === 0) return tr("{{count}} hr", { count: hours });
  return tr("{{hours}} hr {{minutes}} min", { hours, minutes });
}

function modifierSymbol(): string {
  return /mac|iphone|ipad/i.test(navigator.platform) ? "⌘" : "Ctrl+";
}

function formatRelativeDate(value: string | null): string {
  if (value === null) return tr("Never opened");
  const date = new Date(value);
  const difference = Date.now() - date.getTime();
  const days = Math.floor(difference / 86_400_000);
  if (days <= 0) return tr("Today");
  if (days === 1) return tr("Yesterday");
  if (days < 7) return tr("{{count}} days ago", { count: days });
  return new Intl.DateTimeFormat(locale(), {
    month: "short",
    day: "numeric",
  }).format(date);
}

function itemSubtitle(item: LibraryItem): string {
  if (item.series !== null && item.series !== item.title) {
    return item.number !== null ? `${item.series} · ${item.number}` : item.series;
  }
  return tr("{{count}} pages", {
    count: item.pageCount,
  });
}

function visibleItems(
  items: LibraryItem[],
  route: LibraryRoute,
  search: string,
  sortMode: LibrarySortMode,
): LibraryItem[] {
  const query = search.trim().toLocaleLowerCase();
  const visible = items
    .filter((item) => {
      if (route === "hidden") return item.isHidden;
      if (item.isHidden) return false;
      if (route === "continue") {
        return item.progress > 0 && !item.isCompleted;
      }
      if (route === "favorites") return item.isFavorite;
      return true;
    })
    .filter((item) => {
      if (query.length === 0) return true;
      return (
        item.title.toLocaleLowerCase().includes(query) ||
        item.series?.toLocaleLowerCase().includes(query) === true ||
        item.path.toLocaleLowerCase().includes(query)
      );
    });
  return visible.sort((left, right) => {
    if (sortMode === "title") {
      return left.title.localeCompare(right.title, undefined, {
        numeric: true,
        sensitivity: "base",
      });
    }
    if (sortMode === "series") {
      return (left.series ?? left.title).localeCompare(
        right.series ?? right.title,
        undefined,
        { numeric: true, sensitivity: "base" },
      );
    }
    if (sortMode === "added") return right.addedAt.localeCompare(left.addedAt);
    if (sortMode === "progress") return right.progress - left.progress;
    return (right.lastOpenedAt ?? right.addedAt).localeCompare(
      left.lastOpenedAt ?? left.addedAt,
    );
  });
}

export function AppShell() {
  const route = useKomaStore((state) => state.route);
  const items = useKomaStore((state) => state.items);
  const search = useKomaStore((state) => state.search);
  const selectedId = useKomaStore((state) => state.selectedId);
  const sidebarOpen = useKomaStore((state) => state.sidebarOpen);
  const sortMode = useKomaStore((state) => state.sortMode);
  const setSidebarOpen = useKomaStore((state) => state.setSidebarOpen);
  const setSelectedId = useKomaStore((state) => state.setSelectedId);
  const selected = items.find((item) => item.id === selectedId) ?? null;
  const showInspector = route !== "settings" && selected !== null;
  const filtered = visibleItems(items, route, search, sortMode);

  const clearSelectionFromCanvas = (event: ReactPointerEvent<HTMLElement>) => {
    const target = event.target as HTMLElement;
    if (
      target.closest(
        "button, a, input, select, textarea, [role='menuitem'], .book-card, .book-row, .continue-feature",
      )
    ) {
      return;
    }
    setSelectedId(null);
  };

  return (
    <div className="app-shell">
      <Sidebar open={sidebarOpen} />
      {sidebarOpen && (
        <button
          className="sidebar-scrim"
          aria-label={tr("Close navigation")}
          onClick={() => setSidebarOpen(false)}
        />
      )}
      <div className="app-workspace">
        <TopBar />
        <div className={`content-frame${showInspector ? " has-inspector" : ""}`}>
          <main
            className="library-main"
            id="main-content"
            onPointerDown={clearSelectionFromCanvas}
          >
            {route === "settings" ? (
              <SettingsView />
            ) : route === "home" && search.trim().length === 0 ? (
              <HomeView items={filtered} />
            ) : (
              <CollectionView items={filtered} route={route} />
            )}
          </main>
          {showInspector && selected !== null ? (
            <DetailInspector item={selected} />
          ) : null}
        </div>
      </div>
      <BottomNavigation />
    </div>
  );
}

function Sidebar({ open }: { open: boolean }) {
  const route = useKomaStore((state) => state.route);
  const items = useKomaStore((state) => state.items);
  const setRoute = useKomaStore((state) => state.setRoute);
  const openImport = useKomaStore((state) => state.setImportOpen);
  const hiddenCount = items.filter((item) => item.isHidden).length;
  const continueCount = items.filter(
    (item) => !item.isHidden && item.progress > 0 && !item.isCompleted,
  ).length;

  return (
    <aside
      className={`sidebar${open ? " is-open" : ""}`}
      aria-label="Koma"
      data-tauri-drag-region
    >
      <div className="brand" data-tauri-drag-region>
        <img src="/koma-mark.svg" alt="" data-tauri-drag-region />
        <span data-tauri-drag-region>Koma</span>
      </div>
      <nav className="sidebar-nav" aria-label={tr("Library")}>
        {ROUTES.map((item) => {
          const Icon = item.icon;
          const count =
            item.id === "hidden"
              ? hiddenCount
              : item.id === "continue"
                ? continueCount
                : 0;
          return (
            <button
              type="button"
              className={route === item.id ? "is-active" : ""}
              aria-current={route === item.id ? "page" : undefined}
              onClick={() => setRoute(item.id)}
              key={item.id}
            >
              <Icon size={17} strokeWidth={1.8} aria-hidden="true" />
              <span>{tr(item.label)}</span>
              {count > 0 && <span className="nav-count">{count}</span>}
            </button>
          );
        })}
      </nav>

      <div className="sidebar-spacer" />
      <button
        type="button"
        className="sidebar-import"
        onClick={() => openImport(true)}
      >
        <Import size={16} aria-hidden="true" />
        {tr("Import from link")}
      </button>
      <button
        type="button"
        className={route === "settings" ? "sidebar-settings is-active" : "sidebar-settings"}
        onClick={() => setRoute("settings")}
      >
        <Settings size={17} aria-hidden="true" />
        {tr("Settings")}
      </button>
    </aside>
  );
}

function TopBar() {
  const route = useKomaStore((state) => state.route);
  const search = useKomaStore((state) => state.search);
  const setSearch = useKomaStore((state) => state.setSearch);
  const setSidebarOpen = useKomaStore((state) => state.setSidebarOpen);
  const setCommandOpen = useKomaStore((state) => state.setCommandOpen);
  const addFiles = useKomaStore((state) => state.addFiles);
  const addFolder = useKomaStore((state) => state.addFolder);
  const setImportOpen = useKomaStore((state) => state.setImportOpen);
  const platform = useKomaStore((state) => state.bootstrap?.platform);
  const mobile = platform === "ios" || platform === "android";

  return (
    <header className="topbar" data-tauri-drag-region>
      <div className="topbar-leading" data-tauri-drag-region>
        <button
          className="icon-button mobile-menu"
          type="button"
          aria-label={tr("Open navigation")}
          onClick={() => setSidebarOpen(true)}
        >
          <Menu size={19} />
        </button>
        <h1 data-tauri-drag-region>{routeTitle(route)}</h1>
      </div>
      {route !== "settings" && (
        <div className="search-field" role="search">
          <Search size={16} aria-hidden="true" />
          <input
            data-koma-search
            type="search"
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            placeholder={tr("Search your library")}
            aria-label={tr("Search your library")}
            spellCheck={false}
          />
          {!mobile && (
            <button
              type="button"
              className="search-shortcut"
              onClick={() => setCommandOpen(true)}
              aria-label={tr("Open command search")}
            >
              <span>{modifierSymbol()}</span>K
            </button>
          )}
        </div>
      )}
      <div className="topbar-actions">
        {route !== "settings" && <DropdownMenu.Root>
          <DropdownMenu.Trigger asChild>
            <button
              type="button"
              className="primary-button compact"
              aria-label={tr("Add publications")}
            >
              <Plus size={16} aria-hidden="true" />
              <span>{tr("Add")}</span>
            </button>
          </DropdownMenu.Trigger>
          <DropdownMenu.Portal>
            <DropdownMenu.Content
              className="menu-content"
              sideOffset={8}
              align="end"
            >
              <DropdownMenu.Item
                className="menu-item"
                onSelect={() => void addFiles()}
              >
                <FileArchive size={16} />
                {tr("Add files")}
                {!mobile && <span className="menu-shortcut">{modifierSymbol()}O</span>}
              </DropdownMenu.Item>
              <DropdownMenu.Item
                className="menu-item"
                onSelect={() => void addFolder()}
              >
                <FolderOpen size={16} />
                {tr("Scan folder")}
              </DropdownMenu.Item>
              <DropdownMenu.Separator className="menu-separator" />
              <DropdownMenu.Item
                className="menu-item"
                onSelect={() => setImportOpen(true)}
              >
                <Import size={16} />
                {tr("Import from link")}
              </DropdownMenu.Item>
            </DropdownMenu.Content>
          </DropdownMenu.Portal>
        </DropdownMenu.Root>}
      </div>
    </header>
  );
}

function SortControl({
  value,
  onChange,
}: {
  value: LibrarySortMode;
  onChange: (value: LibrarySortMode) => void;
}) {
  const options: Array<[LibrarySortMode, string]> = [
    ["recent", "Recently opened"],
    ["title", "Title"],
    ["series", "Series"],
    ["added", "Recently added"],
    ["progress", "Progress"],
  ];
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>
        <button type="button" className="secondary-button compact sort-button">
          <ArrowDownAZ size={15} />
          <span>{tr(options.find(([id]) => id === value)?.[1] ?? "Sort")}</span>
        </button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content className="menu-content" sideOffset={8} align="end">
          {options.map(([id, label]) => (
            <DropdownMenu.Item
              className="menu-item"
              onSelect={() => onChange(id)}
              key={id}
            >
              <ArrowDownAZ size={15} />
              {tr(label)}
              {id === value && <span className="menu-check">✓</span>}
            </DropdownMenu.Item>
          ))}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

function ViewModeControl({
  value,
  onChange,
}: {
  value: LibraryViewMode;
  onChange: (value: LibraryViewMode) => void;
}) {
  return (
    <div className="segmented compact-segment" aria-label={tr("Library view")}>
      <button
        type="button"
        className={value === "grid" ? "is-active" : ""}
        aria-label={tr("Cover grid")}
        aria-pressed={value === "grid"}
        onClick={() => onChange("grid")}
      >
        <Grid2X2 size={15} />
      </button>
      <button
        type="button"
        className={value === "list" ? "is-active" : ""}
        aria-label={tr("List")}
        aria-pressed={value === "list"}
        onClick={() => onChange("list")}
      >
        <List size={16} />
      </button>
    </div>
  );
}

function HomeView({ items }: { items: LibraryItem[] }) {
  const continueItems = items
    .filter((item) => item.progress > 0 && !item.isCompleted)
    .sort((left, right) =>
      (right.lastOpenedAt ?? "").localeCompare(left.lastOpenedAt ?? ""),
    );
  const featured = continueItems[0] ?? items[0] ?? null;
  const recent = items
    .filter((item) => item.id !== featured?.id)
    .slice(0, 6);
  const openBook = useKomaStore((state) => state.openBook);
  const setRoute = useKomaStore((state) => state.setRoute);

  if (featured === null) return <EmptyLibrary />;

  return (
    <div className="home-view">
      <section className="continue-feature" aria-labelledby="continue-heading">
        <div className="continue-copy">
          <span className="eyebrow">
            {tr(featured.progress > 0 ? "Continue reading" : "Ready to read")}
          </span>
          <h2 id="continue-heading">{featured.title}</h2>
          <p>
            {featured.progress > 0
              ? `${tr("Page {{current}} of {{total}}", {
                  current: featured.currentPage + 1,
                  total: featured.pageCount,
                })} · ${formatProgress(featured.progress)}`
              : `${tr("{{count}} pages", {
                  count: featured.pageCount,
                })} · ${FORMAT_LABELS[featured.format]}`}
          </p>
          <button
            type="button"
            className="primary-button"
            onClick={() => void openBook(featured)}
          >
            <BookOpen size={17} />
            {tr(featured.progress > 0 ? "Continue reading" : "Start reading")}
          </button>
        </div>
        <button
          type="button"
          className="continue-cover"
          onClick={() => void openBook(featured)}
          aria-label={tr("Open {{title}}", { title: featured.title })}
        >
          {featured.coverDataUrl !== null ? (
            <img src={featured.coverDataUrl} alt="" />
          ) : (
            <CoverFallback item={featured} />
          )}
        </button>
        <div className="continue-line" aria-hidden="true">
          <span style={{ width: formatProgress(featured.progress) }} />
        </div>
      </section>

      <section className="library-section" aria-labelledby="recent-heading">
        <div className="section-heading">
          <div>
            <span className="eyebrow">{tr("Library")}</span>
            <h2 id="recent-heading">{tr("Recently added")}</h2>
          </div>
          <button type="button" className="text-button" onClick={() => setRoute("library")}>
            {tr("View library")}
            <ChevronRight size={15} />
          </button>
        </div>
        <BookCollection items={recent} />
      </section>
    </div>
  );
}

function CollectionView({
  items,
  route,
}: {
  items: LibraryItem[];
  route: LibraryRoute;
}) {
  const search = useKomaStore((state) => state.search);
  const allItems = useKomaStore((state) => state.items);
  const viewMode = useKomaStore((state) => state.viewMode);
  const sortMode = useKomaStore((state) => state.sortMode);
  const setSortMode = useKomaStore((state) => state.setSortMode);
  const setViewMode = useKomaStore((state) => state.setViewMode);
  const descriptor =
    search.trim().length > 0
      ? tr("{{count}} results", { count: items.length })
      : tr("{{visible}} of {{total}} publications", {
          visible: items.length.toLocaleString(locale()),
          total: allItems
            .filter((item) => !item.isHidden)
            .length.toLocaleString(locale()),
        });

  return (
    <div className="collection-view">
      <div className="collection-heading">
        <div>
          <span className="eyebrow">{descriptor}</span>
          <h2>
            {search.trim().length > 0
              ? tr("Results for “{{query}}”", { query: search.trim() })
              : routeTitle(route)}
          </h2>
        </div>
        <div className="collection-controls">
          <SortControl value={sortMode} onChange={setSortMode} />
          <ViewModeControl value={viewMode} onChange={setViewMode} />
        </div>
      </div>
      {items.length === 0 ? (
        <EmptyCollection route={route} searching={search.trim().length > 0} />
      ) : (
        <BookCollection items={items} mode={viewMode} />
      )}
    </div>
  );
}

function BookCollection({
  items,
  mode,
}: {
  items: LibraryItem[];
  mode?: LibraryViewMode;
}) {
  const viewMode = useKomaStore((state) => state.viewMode);
  const actualMode = mode ?? viewMode;
  if (items.length > 80) {
    return <VirtualBookCollection items={items} mode={actualMode} />;
  }
  return (
    <div
      className={actualMode === "grid" ? "book-grid" : "book-list"}
      aria-label={tr("Publications")}
    >
      {items.map((item) =>
        actualMode === "grid" ? (
          <BookCard item={item} key={item.id} />
        ) : (
          <BookRow item={item} key={item.id} />
        ),
      )}
    </div>
  );
}

function VirtualBookCollection({
  items,
  mode,
}: {
  items: LibraryItem[];
  mode: LibraryViewMode;
}) {
  const hostRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);

  useEffect(() => {
    const host = hostRef.current;
    if (host === null) return;
    const updateWidth = () => setWidth(host.getBoundingClientRect().width);
    updateWidth();
    const observer = new ResizeObserver(updateWidth);
    observer.observe(host);
    return () => observer.disconnect();
  }, []);

  const columnGap = 20;
  const columnCount =
    mode === "grid"
      ? Math.max(1, Math.floor((Math.max(width, 138) + columnGap) / (138 + columnGap)))
      : 1;
  const rowCount = Math.ceil(items.length / columnCount);
  const scrollMargin = hostRef.current?.offsetTop ?? 0;
  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () =>
      hostRef.current?.closest<HTMLElement>(".library-main") ?? null,
    estimateSize: () => {
      if (mode === "list") return 68;
      const usableWidth = Math.max(width, 320) - columnGap * (columnCount - 1);
      const cardWidth = usableWidth / columnCount;
      return cardWidth * 1.5 + 70;
    },
    getItemKey: (index) => items[index * columnCount]?.id ?? index,
    initialRect: { width: Math.max(width, 320), height: 720 },
    overscan: 3,
    scrollMargin,
  });

  if (width === 0) {
    const firstPaintItems = items.slice(0, mode === "grid" ? 4 : 10);
    return (
      <div
        ref={hostRef}
        className={`virtual-book-collection ${
          mode === "grid" ? "book-grid" : "book-list"
        }`}
        aria-label={tr("Publications")}
      >
        {firstPaintItems.map((item) =>
          mode === "grid" ? (
            <BookCard item={item} key={item.id} />
          ) : (
            <BookRow item={item} key={item.id} />
          ),
        )}
      </div>
    );
  }

  return (
    <div
      ref={hostRef}
      className={`virtual-book-collection${
        mode === "list" ? " book-list virtual-book-list" : ""
      }`}
      style={{ height: virtualizer.getTotalSize() }}
      aria-label={tr("Publications")}
    >
      {virtualizer.getVirtualItems().map((virtualRow) => {
        const rowItems = items.slice(
          virtualRow.index * columnCount,
          Math.min((virtualRow.index + 1) * columnCount, items.length),
        );
        const position = {
          transform: `translateY(${virtualRow.start - scrollMargin}px)`,
        };
        if (mode === "list") {
          const item = rowItems[0];
          return item === undefined ? null : (
            <div
              ref={virtualizer.measureElement}
              className="virtual-list-row"
              data-index={virtualRow.index}
              style={position}
              key={virtualRow.key}
            >
              <BookRow item={item} />
            </div>
          );
        }
        return (
          <div
            ref={virtualizer.measureElement}
            className="book-grid virtual-grid-row"
            data-index={virtualRow.index}
            style={position}
            key={virtualRow.key}
          >
            {rowItems.map((item) => (
              <BookCard item={item} key={item.id} />
            ))}
          </div>
        );
      })}
    </div>
  );
}

function BookCard({ item }: { item: LibraryItem }) {
  const selectedId = useKomaStore((state) => state.selectedId);
  const openingId = useKomaStore((state) => state.readerOpeningId);
  const setSelectedId = useKomaStore((state) => state.setSelectedId);
  const openBook = useKomaStore((state) => state.openBook);
  const platform = useKomaStore((state) => state.bootstrap?.platform);
  const mobile = platform === "ios" || platform === "android";
  return (
    <article
      className={`book-card${selectedId === item.id ? " is-selected" : ""}`}
      aria-label={item.title}
      tabIndex={0}
      onClick={() => {
        setSelectedId(item.id);
        if (mobile) void openBook(item);
      }}
      onDoubleClick={mobile ? undefined : () => void openBook(item)}
      onKeyDown={(event) => {
        if (event.key === "Enter") void openBook(item);
      }}
    >
      <div className="book-cover-wrap">
        {item.coverDataUrl !== null ? (
          <img className="book-cover" src={item.coverDataUrl} alt="" loading="lazy" />
        ) : (
          <CoverFallback item={item} />
        )}
        <span className="format-badge">{FORMAT_LABELS[item.format]}</span>
        {item.isFavorite && (
          <span className="favorite-mark" role="img" aria-label={tr("Favorite")}>
            <Heart size={14} fill="currentColor" />
          </span>
        )}
        {item.isMissing && (
          <span className="missing-cover">{tr("Source missing")}</span>
        )}
        <div className="book-hover-actions">
          <button
            type="button"
            className="cover-open-button"
            disabled={openingId === item.id || item.isMissing}
            onClick={(event) => {
              event.stopPropagation();
              void openBook(item);
            }}
          >
            <BookOpen size={17} />
            {tr(openingId === item.id ? "Opening…" : "Read")}
          </button>
          <BookMenu item={item} />
        </div>
        {item.progress > 0 && (
          <span className="cover-progress" aria-hidden="true">
            <span style={{ width: formatProgress(item.progress) }} />
          </span>
        )}
      </div>
      <div className="book-copy">
        <h3>{item.title}</h3>
        <p>{itemSubtitle(item)}</p>
      </div>
    </article>
  );
}

function BookRow({ item }: { item: LibraryItem }) {
  const selectedId = useKomaStore((state) => state.selectedId);
  const setSelectedId = useKomaStore((state) => state.setSelectedId);
  const openBook = useKomaStore((state) => state.openBook);
  const platform = useKomaStore((state) => state.bootstrap?.platform);
  const mobile = platform === "ios" || platform === "android";
  return (
    <article
      className={`book-row${selectedId === item.id ? " is-selected" : ""}`}
      tabIndex={0}
      onClick={() => {
        setSelectedId(item.id);
        if (mobile) void openBook(item);
      }}
      onDoubleClick={mobile ? undefined : () => void openBook(item)}
      onKeyDown={(event) => {
        if (event.key === "Enter") void openBook(item);
      }}
    >
      <div className="row-cover">
        {item.coverDataUrl !== null ? (
          <img src={item.coverDataUrl} alt="" loading="lazy" />
        ) : (
          <CoverFallback item={item} />
        )}
      </div>
      <div className="row-title">
        <h3>{item.title}</h3>
        <p>{itemSubtitle(item)}</p>
      </div>
      <span className="row-format">{FORMAT_LABELS[item.format]}</span>
      <div className="row-progress">
        <span>
          {item.progress > 0
            ? `${item.currentPage + 1} / ${item.pageCount}`
            : tr("{{count}} pages", { count: item.pageCount })}
        </span>
        <div>
          <span style={{ width: formatProgress(item.progress) }} />
        </div>
      </div>
      <span className="row-date">{formatRelativeDate(item.lastOpenedAt)}</span>
      <BookMenu item={item} />
    </article>
  );
}

function BookMenu({ item }: { item: LibraryItem }) {
  const setFavorite = useKomaStore((state) => state.setFavorite);
  const setHidden = useKomaStore((state) => state.setHidden);
  const setCompleted = useKomaStore((state) => state.setCompleted);
  const revealItem = useKomaStore((state) => state.revealItem);
  const relinkItem = useKomaStore((state) => state.relinkItem);
  const removeItem = useKomaStore((state) => state.removeItem);
  const setToolsItemId = useKomaStore((state) => state.setToolsItemId);
  const platform = useKomaStore((state) => state.bootstrap?.platform);
  const mobile = platform === "ios" || platform === "android";
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>
        <button
          type="button"
          className="icon-button book-menu-trigger"
          aria-label={tr("More options for {{title}}", { title: item.title })}
          onClick={(event) => event.stopPropagation()}
        >
          <MoreHorizontal size={17} />
        </button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content
          className="menu-content"
          sideOffset={6}
          align="end"
          onClick={(event) => event.stopPropagation()}
        >
          <DropdownMenu.Item
            className="menu-item"
            onSelect={() => void setFavorite(item, !item.isFavorite)}
          >
            <Heart size={16} fill={item.isFavorite ? "currentColor" : "none"} />
            {tr(item.isFavorite ? "Remove favorite" : "Favorite")}
          </DropdownMenu.Item>
          <DropdownMenu.Item
            className="menu-item"
            onSelect={() => void setHidden(item, !item.isHidden)}
          >
            {item.isHidden ? <Eye size={16} /> : <EyeOff size={16} />}
            {tr(item.isHidden ? "Restore to library" : "Move to Hidden")}
          </DropdownMenu.Item>
          <DropdownMenu.Item
            className="menu-item"
            onSelect={() => void setCompleted(item, !item.isCompleted)}
          >
            <CircleCheckBig size={16} />
            {tr(item.isCompleted ? "Mark as unread" : "Mark as read")}
          </DropdownMenu.Item>
          <DropdownMenu.Item
            className="menu-item"
            onSelect={() => setToolsItemId(item.id)}
          >
            <Wrench size={16} />
            {tr("Inspect, edit, and convert")}
          </DropdownMenu.Item>
          {!mobile && (
            <DropdownMenu.Item
              className="menu-item"
              onSelect={() => void revealItem(item)}
            >
              <FolderOpen size={16} />
              {tr("Reveal in folder")}
            </DropdownMenu.Item>
          )}
          {item.isMissing && (
            <DropdownMenu.Item
              className="menu-item"
              onSelect={() => void relinkItem(item)}
            >
              <Link2 size={16} />
              {tr("Find moved source")}
            </DropdownMenu.Item>
          )}
          <DropdownMenu.Separator className="menu-separator" />
          <DropdownMenu.Item
            className="menu-item danger"
            onSelect={() => {
              if (
                window.confirm(
                  tr("Remove “{{title}}” from Koma? The original file will be kept.", {
                    title: item.title,
                  }),
                )
              ) {
                void removeItem(item);
              }
            }}
          >
            <Archive size={16} />
            {tr("Remove from Koma")}
          </DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

function CoverFallback({ item }: { item: LibraryItem }) {
  return (
    <div className="cover-fallback">
      <FileArchive size={28} />
      <strong>{item.title}</strong>
      <span>{FORMAT_LABELS[item.format]}</span>
    </div>
  );
}

function DetailInspector({ item }: { item: LibraryItem }) {
  const openBook = useKomaStore((state) => state.openBook);
  const openingId = useKomaStore((state) => state.readerOpeningId);
  const setFavorite = useKomaStore((state) => state.setFavorite);
  const revealItem = useKomaStore((state) => state.revealItem);
  const relinkItem = useKomaStore((state) => state.relinkItem);
  const setToolsItemId = useKomaStore((state) => state.setToolsItemId);
  return (
    <aside
      className="detail-inspector"
      aria-label={tr("Details for {{title}}", { title: item.title })}
    >
      <div className="inspector-cover">
        {item.coverDataUrl !== null ? (
          <img src={item.coverDataUrl} alt="" />
        ) : (
          <CoverFallback item={item} />
        )}
      </div>
      <span className="eyebrow">{FORMAT_LABELS[item.format]}</span>
      <h2>{item.title}</h2>
      <p className="inspector-series">{itemSubtitle(item)}</p>
      <button
        type="button"
        className="primary-button inspector-open"
        disabled={openingId === item.id}
        onClick={() =>
          void (item.isMissing ? relinkItem(item) : openBook(item))
        }
      >
        {item.isMissing ? <Link2 size={17} /> : <BookOpen size={17} />}
        {item.isMissing
          ? tr("Find moved source")
          : openingId === item.id
          ? tr("Opening…")
          : item.progress > 0 && !item.isCompleted
            ? tr("Continue")
            : item.isCompleted
              ? tr("Read again")
              : tr("Start reading")}
      </button>
      <div className="inspector-progress">
        <div>
          <span>{tr(item.isCompleted ? "Completed" : "Progress")}</span>
          <strong>{formatProgress(item.progress)}</strong>
        </div>
        <div className="progress-track">
          <span style={{ width: formatProgress(item.progress) }} />
        </div>
        <small>
          {tr("Page {{current}} of {{total}}", {
            current: Math.min(item.currentPage + 1, item.pageCount),
            total: item.pageCount,
          })}
        </small>
      </div>
      <dl className="inspector-facts">
        {item.currentChapter !== null && (
          <div>
            <dt>{tr("Chapter")}</dt>
            <dd>{item.currentChapter}</dd>
          </div>
        )}
        <div>
          <dt>{tr("Reading time")}</dt>
          <dd>{formatReadingTime(item.totalReadingSeconds)}</dd>
        </div>
        <div>
          <dt>{tr("Last read")}</dt>
          <dd>{formatRelativeDate(item.lastOpenedAt)}</dd>
        </div>
        <div>
          <dt>{tr("Location")}</dt>
          <dd title={item.path}>{item.path.split(/[\\/]/).slice(-2).join("/")}</dd>
        </div>
        <div>
          <dt>{tr("Status")}</dt>
          <dd className={item.isMissing ? "danger-text" : ""}>
            {tr(item.isMissing ? "Source missing" : "Available offline")}
          </dd>
        </div>
      </dl>
      <TrackingMatcher item={item} />
      <div className="inspector-actions">
        <button
          type="button"
          className="secondary-button"
          onClick={() => void setFavorite(item, !item.isFavorite)}
        >
          <Heart size={16} fill={item.isFavorite ? "currentColor" : "none"} />
          {tr(item.isFavorite ? "Favorited" : "Favorite")}
        </button>
        <button
          type="button"
          className="secondary-button"
          onClick={() => setToolsItemId(item.id)}
        >
          <Wrench size={16} />
          {tr("Tools")}
        </button>
        <button
          type="button"
          className="secondary-button inspector-reveal"
          onClick={() => void revealItem(item)}
        >
          <FolderOpen size={16} />
          {tr("Reveal")}
        </button>
      </div>
    </aside>
  );
}

function EmptyLibrary() {
  const addFiles = useKomaStore((state) => state.addFiles);
  const setImportOpen = useKomaStore((state) => state.setImportOpen);
  return (
    <div className="empty-state">
      <div className="empty-mark">
        <span />
        <span />
        <span />
        <span />
      </div>
      <h2>{tr("Add your first publication")}</h2>
      <p>{tr("Open files, scan a folder, or import from a link.")}</p>
      <div className="empty-actions">
        <button className="primary-button" type="button" onClick={() => void addFiles()}>
          <Plus size={17} />
          {tr("Add publication")}
        </button>
        <button
          className="secondary-button"
          type="button"
          onClick={() => setImportOpen(true)}
        >
          <Import size={17} />
          {tr("Paste link")}
        </button>
      </div>
    </div>
  );
}

function EmptyCollection({
  route,
  searching,
}: {
  route: LibraryRoute;
  searching: boolean;
}) {
  const setSearch = useKomaStore((state) => state.setSearch);
  const setRoute = useKomaStore((state) => state.setRoute);
  const content: Record<LibraryRoute, [string, string]> = {
    home: ["Nothing here yet", "Add a publication to begin."],
    library: ["No publications found", "Add files or scan a folder."],
    continue: ["Nothing in progress", "Start reading to keep your place here."],
    favorites: ["No favorites yet", "Favorite publications appear here."],
    hidden: ["Hidden is empty", "Hidden publications appear here."],
    settings: ["", ""],
  };
  const [title, detail] = content[route];
  return (
    <div className="empty-collection">
      <LibraryBig size={27} />
      <h3>{tr(searching ? "No matching publications" : title)}</h3>
      <p>
        {tr(searching ? "Try another search." : detail)}
      </p>
      {searching ? (
        <button type="button" className="text-button" onClick={() => setSearch("")}>
          {tr("Clear search")}
        </button>
      ) : route !== "library" ? (
        <button type="button" className="text-button" onClick={() => setRoute("library")}>
          {tr("Open library")}
        </button>
      ) : null}
    </div>
  );
}

function SettingsView() {
  const theme = useKomaStore((state) => state.theme);
  const motion = useKomaStore((state) => state.motion);
  const viewMode = useKomaStore((state) => state.viewMode);
  const bootstrap = useKomaStore((state) => state.bootstrap);
  const setTheme = useKomaStore((state) => state.setTheme);
  const setMotion = useKomaStore((state) => state.setMotion);
  const setViewMode = useKomaStore((state) => state.setViewMode);
  const exportBackup = useKomaStore((state) => state.exportBackup);
  const restoreBackup = useKomaStore((state) => state.restoreBackup);
  const setImportOpen = useKomaStore((state) => state.setImportOpen);
  const desktopUpdates =
    backend.kind === "native" &&
    ["macos", "windows", "linux"].includes(bootstrap?.platform ?? "");
  const mobile =
    bootstrap?.platform === "ios" || bootstrap?.platform === "android";

  return (
    <div className="settings-view">
      <div className="settings-intro">
        <h2>{tr("Settings")}</h2>
      </div>
      <SettingsSection title={tr("Appearance")}>
        <SettingRow label={tr("Theme")}>
          <ChoiceControl
            value={theme}
            values={[
              ["system", tr("System")],
              ["light", tr("Light")],
              ["dark", tr("Dark")],
            ]}
            onChange={(value) => setTheme(value as ThemeMode)}
          />
        </SettingRow>
        <SettingRow label={tr("Language")}>
          <LanguageSetting />
        </SettingRow>
        <SettingRow label={tr("Animations")}>
          <ChoiceControl
            value={motion}
            values={[
              ["system", tr("System")],
              ["on", tr("On")],
              ["off", tr("Off")],
            ]}
            onChange={(value) => setMotion(value as MotionMode)}
          />
        </SettingRow>
        <SettingRow label={tr("Default library view")}>
          <ChoiceControl
            value={viewMode}
            values={[
              ["grid", tr("Covers")],
              ["list", tr("List")],
            ]}
            onChange={(value) => setViewMode(value as LibraryViewMode)}
          />
        </SettingRow>
      </SettingsSection>
      {!mobile && (
        <SettingsSection title={tr("Library folders")}>
          <ManagedFolders />
        </SettingsSection>
      )}
      <SettingsSection title={tr("Connectors")}>
        <ConnectorSettings />
      </SettingsSection>
      <SettingsSection title={tr("Library")}>
        <SettingRow label={tr("Backup")}>
          <div className="setting-inline-actions">
            <button
              type="button"
              className="secondary-button"
              onClick={() => void restoreBackup()}
            >
              {tr("Restore")}
            </button>
            <button
              type="button"
              className="secondary-button"
              onClick={() => void exportBackup()}
            >
              <Archive size={16} />
              {tr("Export")}
            </button>
          </div>
        </SettingRow>
        <SettingRow label={tr("Link import")}>
          <button
            type="button"
            className="secondary-button"
            onClick={() => setImportOpen(true)}
          >
            <ShieldCheck size={16} />
            {tr("Open importer")}
          </button>
        </SettingRow>
      </SettingsSection>
      <SettingsSection title={tr("Reading tracking")}>
        <TrackingSettings />
      </SettingsSection>
      {!mobile && (
        <SettingsSection title={tr("Discord")}>
          <DiscordPresenceSetting />
        </SettingsSection>
      )}
      {desktopUpdates && (
        <SettingsSection title={tr("Updates")}>
          <UpdateSetting />
        </SettingsSection>
      )}
      <SettingsSection title={tr("About")}>
        <dl className="build-facts">
          <div>
            <dt>{tr("Version")}</dt>
            <dd>{bootstrap?.appVersion ?? "0.1.0"}</dd>
          </div>
          <div>
            <dt>{tr("Platform")}</dt>
            <dd>{bootstrap?.platform ?? (backend.kind === "native" ? "desktop" : "web")}</dd>
          </div>
          <div>
            <dt>GitHub</dt>
            <dd>
              <a
                href="https://github.com/Pixlox/Koma"
                target="_blank"
                rel="noreferrer"
              >
                github.com/Pixlox/Koma
                <ExternalLink size={14} aria-hidden="true" />
              </a>
            </dd>
          </div>
        </dl>
      </SettingsSection>
    </div>
  );
}

function DiscordPresenceSetting() {
  const [enabled, setEnabled] = useState(
    () => localStorage.getItem("koma.discordPresence") === "true",
  );
  const [error, setError] = useState<string | null>(null);

  const change = async (next: boolean) => {
    setError(null);
    try {
      await backend.setDiscordPresence(
        next,
        tr("Browsing the library"),
        tr("Choosing what to read"),
      );
      localStorage.setItem("koma.discordPresence", String(next));
      setEnabled(next);
    } catch (caught) {
      setError(errorMessage(caught));
      setEnabled(false);
      localStorage.setItem("koma.discordPresence", "false");
    }
  };

  return (
    <SettingRow label={tr("Rich Presence")}>
      <div className="setting-with-error">
        <label className="switch-control">
          <input
            type="checkbox"
            checked={enabled}
            onChange={(event) => void change(event.target.checked)}
            aria-label={tr("Discord Rich Presence")}
          />
          <span aria-hidden="true"><span /></span>
        </label>
        {error !== null && <span className="danger-text">{error}</span>}
      </div>
    </SettingRow>
  );
}

const TRACKING_PROVIDERS: Array<{
  id: TrackingProvider;
  label: string;
}> = [
  { id: "aniList", label: "AniList" },
  { id: "myAnimeList", label: "MyAnimeList" },
];

function trackingStatus(status: string | null | undefined): string | null {
  if (status === null || status === undefined) return null;
  const key = status.toLocaleLowerCase().replaceAll("_", "");
  const labels: Record<string, string> = {
    current: "Reading",
    reading: "Reading",
    completed: "Completed",
    paused: "On hold",
    onhold: "On hold",
    dropped: "Dropped",
    planning: "Plan to read",
    plantoread: "Plan to read",
    repeating: "Rereading",
    rereading: "Rereading",
  };
  return tr(labels[key] ?? "Unknown");
}

function trackingProviderLabel(provider: TrackingProvider): string {
  return (
    TRACKING_PROVIDERS.find((candidate) => candidate.id === provider)?.label ??
    provider
  );
}

function TrackingSettings() {
  const [accounts, setAccounts] = useState<TrackingAccount[]>([]);
  const [busy, setBusy] = useState<TrackingProvider | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const refresh = () => {
    void backend
      .trackingAccounts()
      .then(setAccounts)
      .catch((caught) => setError(errorMessage(caught)));
  };

  useEffect(() => {
    let active = true;
    refresh();
    let unlisten: (() => void) | null = null;
    const handleAuth = (event: TrackingAuthEvent) => {
      if (!active) return;
      setBusy(null);
      if (event.success) {
        setError(null);
        setNotice(tr("Account connected"));
        refresh();
      } else {
        setNotice(null);
        setError(
          localizeMessage(
            event.message,
            "Account connection could not be completed.",
          ),
        );
      }
    };
    void backend
      .onTrackingAuth(handleAuth)
      .then((next) => {
        if (!active) {
          next();
          return;
        }
        unlisten = next;
        void backend.takeTrackingAuth().then((event) => {
          if (event !== null) handleAuth(event);
        });
      });
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  const connect = async (provider: TrackingProvider) => {
    setBusy(provider);
    setError(null);
    setNotice(null);
    try {
      await backend.beginTrackingOAuth(provider);
      setBusy(null);
    } catch (caught) {
      setError(errorMessage(caught));
      setBusy(null);
    }
  };

  const disconnect = async (provider: TrackingProvider) => {
    setBusy(provider);
    setError(null);
    try {
      await backend.disconnectTracking(provider);
      refresh();
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="tracking-settings">
      {TRACKING_PROVIDERS.map(({ id, label }) => {
        const account = accounts.find((candidate) => candidate.provider === id);
        return (
          <SettingRow
            key={id}
            label={label}
            detail={
              account?.connected
                ? tr("Connected as {{name}}", {
                    name: account.username ?? label,
                  })
                : account !== undefined && !account.oauthConfigured
                  ? tr("OAuth is not configured in this build.")
                : tr("Sync completed chapters to {{name}}.", { name: label })
            }
          >
            {account?.connected ? (
              <button
                type="button"
                className="secondary-button"
                disabled={busy === id}
                onClick={() => void disconnect(id)}
              >
                {tr("Disconnect")}
              </button>
            ) : (
              <button
                type="button"
                className="secondary-button"
                disabled={
                  busy === id ||
                  account === undefined ||
                  !account.oauthConfigured
                }
                onClick={() => void connect(id)}
              >
                {busy === id ? tr("Opening…") : tr("Connect")}
              </button>
            )}
          </SettingRow>
        );
      })}
      {notice !== null && <span className="success-text">{notice}</span>}
      {error !== null && <span className="danger-text">{error}</span>}
    </div>
  );
}

function TrackingMatcher({ item }: { item: LibraryItem }) {
  const notify = useKomaStore((state) => state.notify);
  const [accounts, setAccounts] = useState<TrackingAccount[]>([]);
  const [mappings, setMappings] = useState<TrackingMapping[]>([]);
  const [remoteProgress, setRemoteProgress] = useState<
    TrackingRemoteProgress[]
  >([]);
  const [candidates, setCandidates] = useState<
    Partial<Record<TrackingProvider, TrackingCandidate[]>>
  >({});
  const [editing, setEditing] = useState<TrackingProvider | null>(null);
  const [lookupState, setLookupState] = useState<
    Partial<Record<TrackingProvider, "loading" | "ready" | "empty">>
  >({});
  const [refreshing, setRefreshing] = useState(false);

  const notifyLookupFailure = useCallback(
    (provider?: TrackingProvider) => {
      notify(
        provider === undefined
          ? tr("Reading tracking could not be completed.")
          : tr("Couldn’t search {{name}}", {
              name: trackingProviderLabel(provider),
            }),
        tr("Please try again."),
        "warning",
      );
    },
    [notify],
  );

  const refreshProgress = () => {
    setRefreshing(true);
    return backend
      .trackingRemoteProgress(item.id)
      .then(setRemoteProgress)
      .catch(() => notifyLookupFailure())
      .finally(() => setRefreshing(false));
  };

  const loadCandidates = (provider: TrackingProvider) => {
    setEditing(provider);
    setLookupState((current) => ({ ...current, [provider]: "loading" }));
    void backend
      .suggestTracking(provider, item.series ?? item.title)
      .then((suggestion) => {
        setCandidates((current) => ({
          ...current,
          [provider]: suggestion.candidates,
        }));
        setLookupState((current) => ({
          ...current,
          [provider]:
            suggestion.candidates.length === 0 ? "empty" : "ready",
        }));
      })
      .catch(() => {
        setCandidates((current) => ({ ...current, [provider]: [] }));
        setLookupState((current) => ({ ...current, [provider]: "empty" }));
        notifyLookupFailure(provider);
      });
  };

  useEffect(() => {
    let active = true;
    setEditing(null);
    setRemoteProgress([]);
    setCandidates({});
    setLookupState({});
    void Promise.all([
      backend.trackingAccounts(),
      backend.trackingMappings(item.id),
    ])
      .then(async ([nextAccounts, nextMappings]) => {
        if (!active) return;
        setAccounts(nextAccounts);
        setMappings(nextMappings);
        const connected = nextAccounts.filter((account) => account.connected);
        const unmatched = connected.filter(
          (account) =>
            !nextMappings.some(
              (mapping) => mapping.provider === account.provider,
            ),
        );
        setLookupState(
          Object.fromEntries(
            unmatched.map((account) => [account.provider, "loading"]),
          ),
        );
        const suggestions = await Promise.all(
          unmatched.map(async (account) => {
            try {
              const suggestion = await backend.suggestTracking(
                account.provider,
                item.series ?? item.title,
              );
              const first = suggestion.candidates[0];
              if (suggestion.automatic && first !== undefined) {
                const mapping = {
                  publicationId: item.id,
                  provider: account.provider,
                  mediaId: first.id,
                  mediaTitle: first.title,
                };
                await backend.setTrackingMapping(mapping);
                return {
                  provider: account.provider,
                  mapping,
                  candidates: [],
                  failed: false,
                };
              }
              return {
                provider: account.provider,
                mapping: null,
                candidates: suggestion.candidates,
                failed: false,
              };
            } catch {
              return {
                provider: account.provider,
                mapping: null,
                candidates: [],
                failed: true,
              };
            }
          }),
        );
        if (!active) return;
        suggestions
          .filter((result) => result.failed)
          .forEach((result) => notifyLookupFailure(result.provider));
        setMappings((current) => [
          ...current,
          ...suggestions.flatMap((result) =>
            result.mapping === null ? [] : [result.mapping],
          ),
        ]);
        setCandidates(
          Object.fromEntries(
            suggestions.map((result) => [result.provider, result.candidates]),
          ),
        );
        setLookupState(
          Object.fromEntries(
            suggestions.map((result) => [
              result.provider,
              result.candidates.length === 0 ? "empty" : "ready",
            ]),
          ),
        );
        try {
          const progress = await backend.trackingRemoteProgress(item.id);
          if (active) setRemoteProgress(progress);
        } catch {
          if (active) notifyLookupFailure();
        }
      })
      .catch(() => {
        if (active) notifyLookupFailure();
      });
    return () => {
      active = false;
    };
  }, [item.id, item.series, item.title, notifyLookupFailure]);

  const connected = accounts.filter((account) => account.connected);
  if (connected.length === 0) return null;

  return (
    <div className="tracking-matcher">
      <span className="eyebrow">{tr("Reading tracking")}</span>
      <button
        type="button"
        className="icon-button tracking-refresh"
        aria-label={tr("Refresh")}
        title={tr("Refresh")}
        disabled={refreshing}
        onClick={() => void refreshProgress()}
      >
        <RefreshCw size={13} className={refreshing ? "spin" : undefined} />
      </button>
      {connected.map((account) => {
        const mapping = mappings.find(
          (candidate) => candidate.provider === account.provider,
        );
        const options = candidates[account.provider] ?? [];
        const label = trackingProviderLabel(account.provider);
        const progress = remoteProgress.find(
          (candidate) =>
            candidate.provider === account.provider &&
            candidate.mediaId === mapping?.mediaId,
        );
        if (mapping !== undefined && editing !== account.provider) {
          const status = trackingStatus(progress?.status);
          return (
            <div className="tracking-match" key={account.provider}>
              <span>{label}</span>
              <div className="tracking-match-details">
                <strong>{mapping.mediaTitle}</strong>
                {progress !== undefined && (
                  <span
                    className="tracking-progress"
                    title={
                      progress.updatedAt === null
                        ? undefined
                        : new Date(progress.updatedAt).toLocaleString(locale())
                    }
                  >
                    {tr("Chapter")} {progress.progress}
                    {progress.totalChapters !== null
                      ? ` / ${progress.totalChapters}`
                      : ""}
                    {status !== undefined ? ` · ${status}` : ""}
                  </span>
                )}
              </div>
              <div className="tracking-match-actions">
                <button
                  type="button"
                  className="text-button"
                  onClick={() => loadCandidates(account.provider)}
                >
                  {tr("Change")}
                </button>
                <button
                  type="button"
                  className="text-button danger-text"
                  onClick={() => {
                    void backend
                      .removeTrackingMapping(item.id, account.provider)
                      .then(() => {
                        setMappings((current) =>
                          current.filter(
                            (candidate) =>
                              candidate.provider !== account.provider,
                          ),
                        );
                        setRemoteProgress((current) =>
                          current.filter(
                            (candidate) =>
                              candidate.provider !== account.provider,
                          ),
                        );
                        loadCandidates(account.provider);
                      })
                      .catch(() => notifyLookupFailure());
                  }}
                >
                  {tr("Unlink")}
                </button>
              </div>
            </div>
          );
        }
        if (lookupState[account.provider] === "loading") {
          return (
            <div className="tracking-match" key={account.provider}>
              <span>{label}</span>
              <span className="tracking-empty">{tr("Matching…")}</span>
            </div>
          );
        }
        if (options.length === 0) {
          return (
            <div className="tracking-match" key={account.provider}>
              <span>{label}</span>
              <span className="tracking-empty">{tr("None found.")}</span>
            </div>
          );
        }
        return (
          <label className="tracking-match" key={account.provider}>
            <span>{label}</span>
            <select
              value=""
              aria-label={tr("Match {{title}} on {{name}}", {
                title: item.title,
                name: label,
              })}
              onChange={(event) => {
                const selected = options.find(
                  (candidate) => candidate.id === Number(event.target.value),
                );
                if (selected === undefined) return;
                const next = {
                  publicationId: item.id,
                  provider: account.provider,
                  mediaId: selected.id,
                  mediaTitle: selected.title,
                };
                void backend.setTrackingMapping(next).then(() => {
                  setMappings((current) => [
                    ...current.filter(
                      (candidate) =>
                        candidate.provider !== account.provider,
                    ),
                    next,
                  ]);
                  setEditing(null);
                  void refreshProgress();
                }).catch(() => notifyLookupFailure());
              }}
            >
              <option value="">{tr("Choose title")}</option>
              {options.map((candidate) => (
                <option key={candidate.id} value={candidate.id}>
                  {candidate.title}
                </option>
              ))}
            </select>
          </label>
        );
      })}
    </div>
  );
}

function LanguageSetting() {
  const language = useKomaStore((state) => state.language);
  const setLanguage = useKomaStore((state) => state.setLanguage);

  return (
    <label className="setting-select language-select">
      <Languages size={16} aria-hidden="true" />
      <select
        value={language}
        onChange={(event) => setLanguage(event.target.value)}
        aria-label={tr("Language")}
      >
        <option value="system">{tr("System")}</option>
        {listAvailableLanguages().map((available) => (
          <option value={available.locale} key={available.locale}>
            {available.nativeName}
          </option>
        ))}
      </select>
      <ChevronDown className="select-chevron" size={14} aria-hidden="true" />
    </label>
  );
}

function ConnectorSettings() {
  const connectors = useKomaStore((state) => state.connectors);
  const importConnector = useKomaStore((state) => state.importConnector);
  const removeConnector = useKomaStore(
    (state) => state.removeConnectorPackage,
  );

  return (
    <div className="connector-settings">
      <div className="connector-actions">
        <button
          type="button"
          className="secondary-button"
          onClick={() => void importConnector()}
        >
          <Link2 size={16} />
          {tr("Import connector")}
        </button>
      </div>
      <div className="connector-list">
        {connectors.map((connector) => (
          <div className="connector-row" key={connector.id}>
            <div className="connector-copy">
              <strong>{connector.name}</strong>
              <span>
                {connector.kind === "bundled"
                  ? tr("Bundled")
                  : `v${connector.version}`}
                {connector.runsCode ? " · Rhai" : ""}
                {" · "}
                {connector.capabilities
                  .map((capability) =>
                    capability === "series"
                      ? tr("Series")
                      : capability === "chapter"
                        ? tr("Chapter")
                        : tr("Volume"),
                  )
                  .join(", ")}
              </span>
            </div>
            {connector.removable && (
              <button
                type="button"
                className="icon-button"
                onClick={() => void removeConnector(connector)}
                aria-label={tr("Remove {{name}}", { name: connector.name })}
                title={tr("Remove connector")}
              >
                <Trash2 size={16} />
              </button>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

function ManagedFolders() {
  const folders = useKomaStore((state) => state.libraryFolders);
  const addFolder = useKomaStore((state) => state.addManagedFolder);
  const updateFolder = useKomaStore((state) => state.updateManagedFolder);
  const removeFolder = useKomaStore((state) => state.removeManagedFolder);
  const scanFolder = useKomaStore((state) => state.scanManagedFolder);

  return (
    <div className="managed-folders">
      <div className="managed-folder-actions">
        <button
          type="button"
          className="secondary-button"
          onClick={() => void addFolder()}
        >
          <FolderPlus size={16} />
          {tr("Add folder")}
        </button>
      </div>
      {folders.length === 0 ? (
        <p className="setting-empty">{tr("No library folders.")}</p>
      ) : (
        <div className="managed-folder-list">
          {folders.map((folder) => (
            <ManagedFolderRow
              folder={folder}
              onUpdate={updateFolder}
              onRemove={removeFolder}
              onScan={scanFolder}
              key={folder.id}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function ManagedFolderRow({
  folder,
  onUpdate,
  onRemove,
  onScan,
}: {
  folder: LibraryFolder;
  onUpdate: (
    folder: LibraryFolder,
    patch: Partial<Pick<LibraryFolder, "enabled" | "scanIntervalMinutes">>,
  ) => Promise<void>;
  onRemove: (folder: LibraryFolder) => Promise<void>;
  onScan: (folder: LibraryFolder) => Promise<void>;
}) {
  const lastScan =
    folder.lastScannedAt === null
      ? tr("Not scanned")
      : new Intl.DateTimeFormat(locale(), {
          dateStyle: "medium",
          timeStyle: "short",
        }).format(new Date(folder.lastScannedAt));
  return (
    <div className="managed-folder-row">
      <div className="managed-folder-copy">
        <strong title={folder.path}>{folder.path}</strong>
        <span>
          {folder.lastError === null
            ? tr("Last scan: {{date}}", {
                date: lastScan,
              })
            : localizeMessage(
                folder.lastError,
                "The last folder scan could not be completed.",
              )}
        </span>
      </div>
      <label className="setting-select compact">
        <select
          value={folder.scanIntervalMinutes}
          disabled={!folder.enabled}
          onChange={(event) =>
            void onUpdate(folder, {
              scanIntervalMinutes: Number(event.target.value),
            })
          }
          aria-label={tr("Scan frequency for {{folder}}", {
            folder: folder.path,
          })}
        >
          <option value={15}>{tr("Every 15 minutes")}</option>
          <option value={60}>{tr("Hourly")}</option>
          <option value={360}>{tr("Every 6 hours")}</option>
          <option value={1440}>{tr("Daily")}</option>
        </select>
      </label>
      <label className="switch-control">
        <input
          type="checkbox"
          checked={folder.enabled}
          onChange={(event) =>
            void onUpdate(folder, { enabled: event.target.checked })
          }
          aria-label={tr("Watch {{folder}}", { folder: folder.path })}
        />
        <span aria-hidden="true"><span /></span>
      </label>
      <button
        type="button"
        className="icon-button"
        onClick={() => void onScan(folder)}
        aria-label={tr("Scan now")}
        title={tr("Scan now")}
      >
        <RefreshCw size={16} />
      </button>
      <button
        type="button"
        className="icon-button"
        onClick={() => void onRemove(folder)}
        aria-label={tr("Remove folder")}
        title={tr("Remove folder")}
      >
        <Trash2 size={16} />
      </button>
    </div>
  );
}

function UpdateSetting() {
  const [phase, setPhase] = useState<
    "idle" | "checking" | "available" | "downloading" | "ready" | "current" | "error"
  >("idle");
  const [available, setAvailable] = useState<AvailableUpdate | null>(null);
  const [progress, setProgress] = useState(0);
  const [error, setError] = useState<string | null>(null);

  const check = async () => {
    setPhase("checking");
    setError(null);
    try {
      const result = await checkForUpdate();
      setAvailable(result);
      setPhase(result === null ? "current" : "available");
    } catch (caught) {
      setError(errorMessage(caught));
      setPhase("error");
    }
  };

  const install = async () => {
    if (available === null) return;
    setPhase("downloading");
    let downloaded = 0;
    let total = 0;
    try {
      await available.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? 0;
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          setProgress(total > 0 ? Math.min(100, Math.round((downloaded / total) * 100)) : 0);
        } else {
          setProgress(100);
        }
      });
      setPhase("ready");
    } catch (caught) {
      setError(errorMessage(caught));
      setPhase("error");
    }
  };

  return (
    <div className="update-setting">
      <div>
        <strong>
          {phase === "available"
            ? tr("Koma {{version}} is available", {
                version: available?.info.version,
              })
            : phase === "ready"
              ? tr("Update installed")
              : phase === "current"
                ? tr("Koma is up to date")
                : phase === "error"
                  ? tr("Update check failed")
                  : tr("Automatic desktop updates")}
        </strong>
        {(phase === "checking" || phase === "downloading") && (
          <span>
            {phase === "checking"
              ? tr("Checking for updates…")
              : tr("Downloading… {{progress}}%", { progress })}
          </span>
        )}
        {error !== null && <span className="danger-text">{error}</span>}
      </div>
      {phase === "available" ? (
        <button type="button" className="primary-button" onClick={() => void install()}>
          <Sparkles size={16} />
          {tr("Install update")}
        </button>
      ) : phase === "ready" ? (
        <button
          type="button"
          className="primary-button"
          onClick={() => void restartAfterUpdate()}
        >
          <RefreshCw size={16} />
          {tr("Restart Koma")}
        </button>
      ) : (
        <button
          type="button"
          className="secondary-button"
          disabled={phase === "checking" || phase === "downloading"}
          onClick={() => void check()}
        >
          <RefreshCw size={16} />
          {tr("Check for updates")}
        </button>
      )}
    </div>
  );
}

function SettingsSection({
  title,
  detail,
  children,
}: {
  title: string;
  detail?: string;
  children: ReactNode;
}) {
  return (
    <section className="settings-section">
      <div className="settings-section-copy">
        <h3>{title}</h3>
        {detail !== undefined && <p>{detail}</p>}
      </div>
      <div className="settings-rows">{children}</div>
    </section>
  );
}

function SettingRow({
  label,
  detail,
  children,
}: {
  label: string;
  detail?: string;
  children: ReactNode;
}) {
  return (
    <div className="setting-row">
      <div>
        <strong>{label}</strong>
        {detail !== undefined && <p>{detail}</p>}
      </div>
      {children}
    </div>
  );
}

function ChoiceControl({
  value,
  values,
  onChange,
}: {
  value: string;
  values: Array<[string, string]>;
  onChange: (value: string) => void;
}) {
  return (
    <div className="segmented choice-control">
      {values.map(([id, label]) => (
        <button
          type="button"
          className={value === id ? "is-active" : ""}
          aria-pressed={value === id}
          onClick={() => onChange(id)}
          key={id}
        >
          {value === id && <Check size={13} />}
          {label}
        </button>
      ))}
    </div>
  );
}

function BottomNavigation() {
  const route = useKomaStore((state) => state.route);
  const setRoute = useKomaStore((state) => state.setRoute);
  const items = [
    ROUTES[0],
    ROUTES[1],
    ROUTES[2],
    { id: "settings" as const, label: "Settings", icon: Settings },
  ].filter((item): item is NonNullable<typeof item> => item !== undefined);
  return (
    <nav className="bottom-nav" aria-label={tr("Primary navigation")}>
      {items.map((item) => {
        const Icon = item.icon;
        return (
          <button
            type="button"
            className={route === item.id ? "is-active" : ""}
            onClick={() => setRoute(item.id)}
            key={item.id}
          >
            <Icon size={19} />
            <span>{tr(item.label)}</span>
          </button>
        );
      })}
    </nav>
  );
}

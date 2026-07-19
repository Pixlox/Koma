import * as Slider from "@radix-ui/react-slider";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  ArrowLeft,
  Bookmark,
  ChevronDown,
  ChevronLeft,
  Columns2,
  Focus,
  Image as ImageIcon,
  Maximize2,
  MonitorPlay,
  Moon,
  MoveHorizontal,
  PanelRightClose,
  PanelRightOpen,
  Rows3,
  RotateCcw,
  SlidersHorizontal,
  Sun,
  Trash2,
  X,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import {
  type CSSProperties,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
  type WheelEvent as ReactWheelEvent,
  useEffect,
  useRef,
  useState,
} from "react";

import { tr } from "../i18n";
import { backend } from "../lib/backend";
import { useKomaStore } from "../store/koma";
import type {
  Bookmark as BookmarkRecord,
  FitMode,
  PageDescriptor,
  ReaderMode,
  ReaderSettings,
  ReadingDirection,
  WidePagePolicy,
} from "../types";

const MODES: Array<{
  id: ReaderMode;
  label: string;
  icon: typeof ImageIcon;
}> = [
  { id: "singlePage", label: "Single page", icon: ImageIcon },
  { id: "spreads", label: "Spreads", icon: Columns2 },
  { id: "guided", label: "Panel focus", icon: Focus },
  { id: "continuous", label: "Continuous", icon: Rows3 },
  { id: "webtoon", label: "Webtoon", icon: MoveHorizontal },
  { id: "presentation", label: "Presentation", icon: MonitorPlay },
];

function isWidePage(page: PageDescriptor): boolean {
  return (
    page.width !== null &&
    page.height !== null &&
    page.width > page.height * 1.15
  );
}

function spreadGroups(pages: PageDescriptor[]): number[][] {
  const groups: number[][] = [];
  let index = 0;
  while (index < pages.length) {
    const page = pages[index];
    if (page === undefined) break;
    const next = pages[index + 1];
    if (
      page.isCover ||
      isWidePage(page) ||
      next === undefined ||
      next.isCover ||
      isWidePage(next)
    ) {
      groups.push([index]);
      index += 1;
    } else {
      groups.push([index, index + 1]);
      index += 2;
    }
  }
  return groups;
}

function spreadIndexForPage(groups: number[][], page: number): number {
  const exact = groups.findIndex((group) => group.includes(page));
  if (exact >= 0) return exact;
  return Math.max(0, groups.length - 1);
}

function resolvedDirection(
  direction: ReadingDirection,
  metadataDirection: ReadingDirection,
): ReadingDirection {
  if (direction !== "automatic") return direction;
  return metadataDirection === "automatic" ? "leftToRight" : metadataDirection;
}

function compactPhoneViewport(): boolean {
  const media = window.matchMedia?.("(max-width: 560px)");
  return media?.matches ?? window.innerWidth <= 560;
}

export function Reader() {
  const reader = useKomaStore((state) => state.reader);
  const closeReader = useKomaStore((state) => state.closeReader);
  const goToPage = useKomaStore((state) => state.goToPage);
  const loadPage = useKomaStore((state) => state.loadPage);
  const setControls = useKomaStore((state) => state.setReaderControls);
  const setSettingsOpen = useKomaStore((state) => state.setReaderSettingsOpen);
  const updateSettings = useKomaStore((state) => state.updateReaderSettings);
  const setZoom = useKomaStore((state) => state.setReaderZoom);
  const toggleBookmark = useKomaStore((state) => state.toggleReaderBookmark);
  const saveAnnotation = useKomaStore((state) => state.saveReaderAnnotation);
  const removeBookmark = useKomaStore((state) => state.removeReaderBookmark);
  const notify = useKomaStore((state) => state.notify);
  const platform = useKomaStore((state) => state.bootstrap?.platform);
  const mobile = platform === "ios" || platform === "android";
  const hideTimer = useRef<number | null>(null);
  const wheelNavigationAt = useRef(0);
  const stageRef = useRef<HTMLDivElement>(null);
  const pointers = useRef(new Map<number, { x: number; y: number }>());
  const gestureStart = useRef<{
    pointerId: number;
    x: number;
    y: number;
    panX: number;
    panY: number;
    moved: boolean;
  } | null>(null);
  const pinchStart = useRef<{
    distance: number;
    zoom: number;
    centerX: number;
    centerY: number;
  } | null>(null);
  const [guidedStep, setGuidedStep] = useState(0);
  const [pan, setPan] = useState({ x: 0, y: 0 });
  const [fullscreen, setFullscreen] = useState(false);
  const [compactPhone, setCompactPhone] = useState(compactPhoneViewport);

  const manifest = reader?.payload.manifest ?? null;
  const pageCount = manifest?.pages.length ?? 0;
  const readerId = reader?.payload.libraryId;
  const currentPage = reader?.currentPage;
  const currentChapter =
    manifest?.chapters.find(
      (chapter) =>
        currentPage !== undefined &&
        currentPage >= chapter.startPageIndex &&
        currentPage <= chapter.endPageIndex,
    ) ?? null;
  const readerMode =
    compactPhone && reader?.settings.mode === "spreads"
      ? "singlePage"
      : reader?.settings.mode;
  const keepAwake = reader?.settings.keepAwake ?? false;
  const direction =
    reader === null || manifest === null
      ? "leftToRight"
      : resolvedDirection(reader.settings.direction, manifest.metadata.direction);
  const isRtl = direction === "rightToLeft";

  const previous = () => {
    if (reader === null) return;
    if (reader.settings.mode === "guided" && guidedStep > 0) {
      setGuidedStep((step) => step - 1);
      return;
    }
    if (reader.settings.mode === "guided" && reader.currentPage > 0) {
      setGuidedStep(3);
    }
    if (readerMode === "spreads" && manifest !== null) {
      const groups = spreadGroups(manifest.pages);
      const index = spreadIndexForPage(groups, reader.currentPage);
      const target = groups[Math.max(0, index - 1)]?.[0];
      if (target !== undefined) void goToPage(target);
      return;
    }
    void goToPage(reader.currentPage - 1);
  };
  const next = () => {
    if (reader === null) return;
    if (reader.settings.mode === "guided" && guidedStep < 3) {
      setGuidedStep((step) => step + 1);
      return;
    }
    if (reader.settings.mode === "guided") setGuidedStep(0);
    if (readerMode === "spreads" && manifest !== null) {
      const groups = spreadGroups(manifest.pages);
      const index = spreadIndexForPage(groups, reader.currentPage);
      const target = groups[Math.min(groups.length - 1, index + 1)]?.[0];
      if (target !== undefined) void goToPage(target);
      return;
    }
    void goToPage(reader.currentPage + 1);
  };

  const scheduleHide = () => {
    if (hideTimer.current !== null) window.clearTimeout(hideTimer.current);
    if (reader !== null && !reader.settingsOpen) {
      hideTimer.current = window.setTimeout(() => setControls(false), 2_600);
    }
  };

  useEffect(() => {
    const media = window.matchMedia?.("(max-width: 560px)");
    const sync = () => setCompactPhone(media?.matches ?? window.innerWidth <= 560);
    sync();
    if (media === undefined) {
      window.addEventListener("resize", sync);
      return () => window.removeEventListener("resize", sync);
    }
    media.addEventListener("change", sync);
    return () => media.removeEventListener("change", sync);
  }, []);

  useEffect(() => {
    if (readerId === undefined) return;
    let lastActivityAt = Date.now();
    let lastRecordedAt = Date.now();
    const markActive = () => {
      lastActivityAt = Date.now();
    };
    const flush = () => {
      const now = Date.now();
      if (
        document.visibilityState !== "visible" ||
        now - lastActivityAt > 120_000
      ) {
        lastRecordedAt = now;
        return;
      }
      const elapsedSeconds = Math.floor((now - lastRecordedAt) / 1000);
      lastRecordedAt = now;
      if (elapsedSeconds > 0) {
        void backend.recordReadingTime(readerId, elapsedSeconds);
      }
    };
    const interval = window.setInterval(flush, 15_000);
    window.addEventListener("pointerdown", markActive, { passive: true });
    window.addEventListener("keydown", markActive);
    window.addEventListener("wheel", markActive, { passive: true });
    document.addEventListener("visibilitychange", flush);
    return () => {
      window.clearInterval(interval);
      flush();
      window.removeEventListener("pointerdown", markActive);
      window.removeEventListener("keydown", markActive);
      window.removeEventListener("wheel", markActive);
      document.removeEventListener("visibilitychange", flush);
    };
  }, [readerId]);

  useEffect(() => {
    if (compactPhone && reader?.settings.mode === "spreads") {
      void updateSettings({ mode: "singlePage" });
    }
  }, [compactPhone, reader?.settings.mode, updateSettings]);

  const showControls = () => {
    if (reader === null) return;
    setControls(true);
    scheduleHide();
  };

  const clampPan = (
    next: { x: number; y: number },
    zoom: number,
  ): { x: number; y: number } => {
    if (zoom <= 1) return { x: 0, y: 0 };
    const bounds = stageRef.current?.getBoundingClientRect();
    if (bounds === undefined) return next;
    const margin = 32;
    const maxX = (bounds.width * (zoom - 1)) / 2 + margin;
    const maxY = (bounds.height * (zoom - 1)) / 2 + margin;
    return {
      x: Math.max(-maxX, Math.min(maxX, next.x)),
      y: Math.max(-maxY, Math.min(maxY, next.y)),
    };
  };

  const setZoomAround = (nextZoom: number, clientX?: number, clientY?: number) => {
    if (reader === null) return;
    const bounded = Math.max(0.25, Math.min(5, nextZoom));
    const stage = stageRef.current;
    if (
      stage !== null &&
      clientX !== undefined &&
      clientY !== undefined &&
      reader.zoom > 0
    ) {
      const bounds = stage.getBoundingClientRect();
      const cursorX = clientX - (bounds.left + bounds.width / 2);
      const cursorY = clientY - (bounds.top + bounds.height / 2);
      const ratio = bounded / reader.zoom;
      setPan((current) =>
        clampPan(
          {
            x: cursorX - (cursorX - current.x) * ratio,
            y: cursorY - (cursorY - current.y) * ratio,
          },
          bounded,
        ),
      );
    } else if (bounded <= 1) {
      setPan({ x: 0, y: 0 });
    }
    setZoom(bounded);
  };

  useEffect(() => {
    if (reader === null) return;
    setGuidedStep(0);
    setPan({ x: 0, y: 0 });
    showControls();
    return () => {
      if (hideTimer.current !== null) window.clearTimeout(hideTimer.current);
    };
    // Opening a different publication is the state transition that resets chrome.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [reader?.payload.libraryId]);

  useEffect(() => {
    setPan({ x: 0, y: 0 });
  }, [reader?.currentPage, reader?.settings.mode]);

  useEffect(() => {
    if (readerId === undefined) return;
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    const syncNativeFullscreen = async () => {
      const current = await getCurrentWindow().isFullscreen();
      if (!cancelled) setFullscreen(current);
    };
    if ("__TAURI_INTERNALS__" in window) {
      void syncNativeFullscreen();
      void getCurrentWindow()
        .onResized(() => {
          void syncNativeFullscreen();
        })
        .then((next) => {
          if (cancelled) next();
          else unlisten = next;
        });
    } else {
      const syncBrowserFullscreen = () =>
        setFullscreen(document.fullscreenElement !== null);
      syncBrowserFullscreen();
      document.addEventListener("fullscreenchange", syncBrowserFullscreen);
      unlisten = () =>
        document.removeEventListener("fullscreenchange", syncBrowserFullscreen);
    }
    const constrainPan = () => {
      setPan((current) => clampPan(current, reader?.zoom ?? 1));
    };
    window.addEventListener("resize", constrainPan);
    return () => {
      cancelled = true;
      unlisten?.();
      window.removeEventListener("resize", constrainPan);
    };
    // Fullscreen tracking belongs to the currently open reader window.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [readerId]);

  useEffect(() => {
    if (readerId === undefined || !keepAwake) return;
    let active = true;
    let sentinel: WakeLockSentinel | null = null;
    const acquire = async () => {
      if (!("wakeLock" in navigator) || document.visibilityState !== "visible") {
        return;
      }
      try {
        sentinel = await navigator.wakeLock.request("screen");
      } catch {
        // Wake Lock is a best-effort platform capability.
      }
    };
    const onVisibilityChange = () => {
      if (active && document.visibilityState === "visible") void acquire();
    };
    void acquire();
    document.addEventListener("visibilitychange", onVisibilityChange);
    return () => {
      active = false;
      document.removeEventListener("visibilitychange", onVisibilityChange);
      void sentinel?.release();
    };
  }, [keepAwake, readerId]);

  useEffect(() => {
    if (reader === null) return;
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target;
      if (
        target instanceof HTMLInputElement ||
        target instanceof HTMLTextAreaElement ||
        target instanceof HTMLSelectElement
      ) {
        return;
      }
      const key = event.key.toLocaleLowerCase();
      let revealControls = false;
      if (event.key === "Escape") {
        event.preventDefault();
        if (reader.settingsOpen) setSettingsOpen(false);
        else if (fullscreen) {
          void setReaderFullscreen(false).then(() => setFullscreen(false));
        } else closeReader();
      } else if (
        event.key === "ArrowRight" ||
        event.key === "ArrowLeft"
      ) {
        event.preventDefault();
        const wantsNext =
          (event.key === "ArrowRight" && !isRtl) ||
          (event.key === "ArrowLeft" && isRtl);
        if (wantsNext) next();
        else previous();
      } else if (event.key === " " || event.key === "PageDown") {
        event.preventDefault();
        next();
      } else if (event.key === "PageUp") {
        event.preventDefault();
        previous();
      } else if (event.key === "Home") {
        event.preventDefault();
        setGuidedStep(0);
        void goToPage(0);
      } else if (event.key === "End") {
        event.preventDefault();
        setGuidedStep(0);
        void goToPage(pageCount - 1);
      } else if (key === "b") {
        event.preventDefault();
        void toggleBookmark();
        revealControls = true;
      } else if (key === "s") {
        event.preventDefault();
        setSettingsOpen(!reader.settingsOpen);
        revealControls = true;
      } else if (key === "f") {
        event.preventDefault();
        void setReaderFullscreen(!fullscreen)
          .then(() => {
            setFullscreen(!fullscreen);
            setControls(false);
          })
          .catch(() =>
            notify(
              tr("Fullscreen unavailable"),
              tr("Could not change fullscreen mode"),
              "danger",
            ),
          );
      } else if (key === "m") {
        event.preventDefault();
        const index = MODES.findIndex((mode) => mode.id === reader.settings.mode);
        const nextMode = MODES[(index + 1) % MODES.length];
        if (nextMode !== undefined) void updateSettings({ mode: nextMode.id });
        revealControls = true;
      } else if (event.key === "+" || event.key === "=") {
        event.preventDefault();
        setZoomAround(reader.zoom * 1.15);
        revealControls = true;
      } else if (event.key === "-") {
        event.preventDefault();
        setZoomAround(reader.zoom / 1.15);
        revealControls = true;
      } else if (event.key === "0" && (event.metaKey || event.ctrlKey)) {
        event.preventDefault();
        setZoomAround(1);
        revealControls = true;
      }
      if (revealControls) showControls();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
    // Functions are stable store actions. Reader state is intentionally current.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [reader, isRtl, pageCount, guidedStep, fullscreen]);

  useEffect(() => {
    if (
      currentPage === undefined ||
      manifest === null ||
      readerMode !== "spreads"
    ) {
      return;
    }
    const groups = spreadGroups(manifest.pages);
    const index = spreadIndexForPage(groups, currentPage);
    for (const page of [
      ...(groups[index] ?? []),
      ...(groups[index + 1] ?? []),
    ]) {
      void loadPage(page);
    }
  }, [currentPage, loadPage, manifest, readerMode]);

  if (reader === null || manifest === null) return null;

  const isBookmarked = reader.bookmarks.some(
    (bookmark) => bookmark.pageIndex === reader.currentPage,
  );
  const filter = [
    reader.settings.gamma !== 1 || reader.settings.sharpen
      ? "url(#koma-reader-image-filter)"
      : "",
    `brightness(${reader.settings.brightness})`,
    `contrast(${reader.settings.contrast})`,
    `saturate(${reader.settings.saturation})`,
    reader.settings.grayscale ? "grayscale(1)" : "",
    reader.settings.invert ? "invert(1)" : "",
  ]
    .filter(Boolean)
    .join(" ");
  const imageStyle: CSSProperties = {
    filter,
    transform: reader.settings.cropMargins ? "scale(1.035)" : undefined,
  };

  const onWheel = (event: ReactWheelEvent) => {
    if (event.ctrlKey || event.metaKey) {
      event.preventDefault();
      const factor = Math.exp(-event.deltaY * 0.0025);
      setZoomAround(reader.zoom * factor, event.clientX, event.clientY);
      return;
    }
    if (
      Math.abs(event.deltaX) > 35 &&
      Math.abs(event.deltaX) > Math.abs(event.deltaY) * 1.2 &&
      Date.now() - wheelNavigationAt.current > 420
    ) {
      event.preventDefault();
      wheelNavigationAt.current = Date.now();
      const wantsNext = (event.deltaX > 0 && !isRtl) || (event.deltaX < 0 && isRtl);
      if (wantsNext) next();
      else previous();
    }
  };

  const onPointerDown = (event: ReactPointerEvent) => {
    if (isReaderControl(event.target)) {
      return;
    }
    pointers.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
    gestureStart.current = {
      pointerId: event.pointerId,
      x: event.clientX,
      y: event.clientY,
      panX: pan.x,
      panY: pan.y,
      moved: false,
    };
    event.currentTarget.setPointerCapture?.(event.pointerId);
    if (pointers.current.size === 2) {
      const [first, second] = [...pointers.current.values()];
      if (first !== undefined && second !== undefined) {
        pinchStart.current = {
          distance: Math.hypot(second.x - first.x, second.y - first.y),
          zoom: reader.zoom,
          centerX: (first.x + second.x) / 2,
          centerY: (first.y + second.y) / 2,
        };
      }
    }
  };

  const onPointerMove = (event: ReactPointerEvent) => {
    const existing = pointers.current.get(event.pointerId);
    if (existing !== undefined) {
      pointers.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
    }
    if (pointers.current.size >= 2 && pinchStart.current !== null) {
      const [first, second] = [...pointers.current.values()];
      if (first !== undefined && second !== undefined) {
        const distance = Math.hypot(second.x - first.x, second.y - first.y);
        if (pinchStart.current.distance > 0) {
          setZoomAround(
            pinchStart.current.zoom * (distance / pinchStart.current.distance),
            pinchStart.current.centerX,
            pinchStart.current.centerY,
          );
        }
      }
      if (gestureStart.current !== null) gestureStart.current.moved = true;
      return;
    }
    const start = gestureStart.current;
    if (
      start !== null &&
      start.pointerId === event.pointerId &&
      reader.zoom > 1 &&
      (event.pointerType === "touch" || event.buttons === 1)
    ) {
      const deltaX = event.clientX - start.x;
      const deltaY = event.clientY - start.y;
      if (Math.hypot(deltaX, deltaY) > 3) start.moved = true;
      setPan(
        clampPan(
          { x: start.panX + deltaX, y: start.panY + deltaY },
          reader.zoom,
        ),
      );
      return;
    }
    if (event.pointerType !== "touch") showControls();
  };

  const onPointerUp = (event: ReactPointerEvent) => {
    const start = gestureStart.current;
    pointers.current.delete(event.pointerId);
    if (pointers.current.size < 2) pinchStart.current = null;
    gestureStart.current = null;
    if (isReaderControl(event.target)) {
      return;
    }
    if (
      start !== null &&
      !start.moved &&
      event.pointerType === "touch" &&
      reader.zoom <= 1
    ) {
      const horizontal = event.clientX - start.x;
      const vertical = event.clientY - start.y;
      if (Math.abs(horizontal) > 48 && Math.abs(horizontal) > Math.abs(vertical) * 1.25) {
        const wantsNext = (horizontal < 0 && !isRtl) || (horizontal > 0 && isRtl);
        if (wantsNext) next();
        else previous();
        return;
      }
    }
    if (start?.moved) return;
    setControls(!reader.controlsVisible);
  };

  const onPointerCancel = (event: ReactPointerEvent) => {
    pointers.current.delete(event.pointerId);
    if (pointers.current.size < 2) pinchStart.current = null;
    gestureStart.current = null;
  };

  return (
    <div
      className={[
        "reader-shell",
        `mode-${readerMode}`,
        reader.controlsVisible ? "controls-visible" : "controls-hidden",
        reader.settingsOpen ? "settings-visible" : "",
        reader.zoom > 1 ? "is-zoomed" : "",
      ].join(" ")}
      onPointerMove={onPointerMove}
      onPointerDown={onPointerDown}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerCancel}
      onWheel={onWheel}
      onDoubleClick={(event) => {
        if (isReaderControl(event.target)) {
          return;
        }
        setZoomAround(reader.zoom > 1.05 ? 1 : 2, event.clientX, event.clientY);
      }}
      aria-label={tr("Reading {{title}}", { title: manifest.metadata.title })}
    >
      <svg className="reader-filter-definitions" aria-hidden="true">
        <filter
          id="koma-reader-image-filter"
          colorInterpolationFilters="sRGB"
          x="-5%"
          y="-5%"
          width="110%"
          height="110%"
        >
          <feComponentTransfer>
            <feFuncR
              type="gamma"
              amplitude="1"
              exponent={1 / reader.settings.gamma}
              offset="0"
            />
            <feFuncG
              type="gamma"
              amplitude="1"
              exponent={1 / reader.settings.gamma}
              offset="0"
            />
            <feFuncB
              type="gamma"
              amplitude="1"
              exponent={1 / reader.settings.gamma}
              offset="0"
            />
          </feComponentTransfer>
          {reader.settings.sharpen && (
            <feConvolveMatrix
              order="3"
              kernelMatrix="0 -1 0 -1 5 -1 0 -1 0"
              preserveAlpha="true"
            />
          )}
        </filter>
      </svg>
      <ReaderToolbar
        title={manifest.metadata.title}
        series={manifest.metadata.series}
        settings={reader.settings}
        settingsOpen={reader.settingsOpen}
        controlsVisible={reader.controlsVisible}
        fullscreen={fullscreen}
        onClose={() => {
          if (fullscreen) {
            void setReaderFullscreen(false).finally(closeReader);
          } else {
            closeReader();
          }
        }}
        onSettings={() => setSettingsOpen(!reader.settingsOpen)}
        onMode={(mode) => {
          setGuidedStep(0);
          if (mode === "presentation" && !fullscreen && !mobile) {
            void setReaderFullscreen(true)
              .then(() => {
                setFullscreen(true);
                setControls(false);
              })
              .catch(() =>
                notify(
                  tr("Fullscreen unavailable"),
                  tr("Could not change fullscreen mode"),
                  "danger",
                ),
              );
          }
          if (mode === "presentation" && mobile) setControls(false);
          void updateSettings({ mode });
        }}
        onFullscreen={() => {
          void setReaderFullscreen(!fullscreen)
            .then(() => {
              setFullscreen(!fullscreen);
              setControls(false);
            })
            .catch(() =>
              notify(
                tr("Fullscreen unavailable"),
                tr("Could not change fullscreen mode"),
                "danger",
              ),
            );
        }}
      />

      <div className="reader-stage" ref={stageRef}>
        {readerMode === "guided" ? (
          <GuidedCanvas
            currentPage={reader.currentPage}
            pageUrl={reader.pageUrls[reader.currentPage]}
            step={guidedStep}
            isRtl={isRtl}
            imageStyle={imageStyle}
            zoom={reader.zoom}
            onPrevious={previous}
            onNext={next}
          />
        ) : readerMode === "continuous" ||
        readerMode === "webtoon" ? (
          <ContinuousPages
            mode={readerMode}
            currentPage={reader.currentPage}
            pageUrls={reader.pageUrls}
            settings={reader.settings}
            imageStyle={imageStyle}
            zoom={reader.zoom}
            onLoad={loadPage}
            onVisible={goToPage}
          />
        ) : (
          <PagedCanvas
            currentPage={reader.currentPage}
            pages={manifest.pages}
            pageUrls={reader.pageUrls}
            spread={readerMode === "spreads"}
            isRtl={isRtl}
            settings={reader.settings}
            imageStyle={imageStyle}
            zoom={reader.zoom}
            pan={pan}
            onPrevious={previous}
            onNext={next}
          />
        )}
      </div>

      {reader.settings.showPageNumber &&
        readerMode !== "continuous" &&
        readerMode !== "webtoon" && (
          <div className="reader-page-number" aria-live="polite">
            {currentChapter !== null &&
              `${tr("Chapter {{number}}", { number: currentChapter.number })} · `}
            {reader.currentPage + 1} / {pageCount}
          </div>
        )}

      <ReaderScrubber
        currentPage={reader.currentPage}
        pageCount={pageCount}
        isRtl={isRtl}
        bookmarked={isBookmarked}
        controlsVisible={reader.controlsVisible}
        onPage={(page) => {
          setGuidedStep(0);
          void goToPage(page);
        }}
        onBookmark={() => void toggleBookmark()}
      />
      <div className="reader-immersive-progress" aria-hidden="true">
        <span
          style={{
            width: `${pageCount <= 1 ? 100 : (reader.currentPage / (pageCount - 1)) * 100}%`,
          }}
        />
      </div>

      <ReaderSettingsPanel
        open={reader.settingsOpen}
        settings={reader.settings}
        zoom={reader.zoom}
        currentPage={reader.currentPage}
        bookmarks={reader.bookmarks}
        metadataDirection={manifest.metadata.direction}
        onChange={(patch) => void updateSettings(patch)}
        onZoom={(zoom) => setZoomAround(zoom)}
        onBookmarkPage={(page) => {
          setGuidedStep(0);
          void goToPage(page);
        }}
        onRemoveBookmark={(id) => void removeBookmark(id)}
        onSaveAnnotation={(label, note) =>
          void saveAnnotation(label, note)
        }
        onClose={() => setSettingsOpen(false)}
      />

      {reader.error !== null && (
        <div className="reader-error" role="alert">
          {reader.error}
        </div>
      )}
    </div>
  );
}

function ReaderToolbar({
  title,
  series,
  settings,
  settingsOpen,
  controlsVisible,
  fullscreen,
  onClose,
  onSettings,
  onMode,
  onFullscreen,
}: {
  title: string;
  series: string | null;
  settings: ReaderSettings;
  settingsOpen: boolean;
  controlsVisible: boolean;
  fullscreen: boolean;
  onClose: () => void;
  onSettings: () => void;
  onMode: (mode: ReaderMode) => void;
  onFullscreen: () => void;
}) {
  const platform = useKomaStore((state) => state.bootstrap?.platform);
  const mobile = platform === "ios" || platform === "android";
  const currentMode = MODES.find((mode) => mode.id === settings.mode) ?? MODES[0]!;
  const CurrentModeIcon = currentMode.icon;
  return (
    <header
      className="reader-toolbar"
      aria-hidden={!controlsVisible}
      inert={!controlsVisible}
      data-fullscreen={fullscreen}
      data-tauri-drag-region
    >
      <div className="reader-toolbar-leading">
        <button
          type="button"
          className="reader-icon-button"
          onClick={onClose}
          aria-label={tr("Close reader")}
        >
          <ChevronLeft size={21} />
        </button>
        <div className="reader-title">
          <strong>{title}</strong>
          {series !== null && series !== title && <span>{series}</span>}
        </div>
      </div>
      <label className="reader-mode-button">
        <CurrentModeIcon size={16} aria-hidden="true" />
        <span className="reader-mode-label">{tr(currentMode.label)}</span>
        <select
          value={settings.mode}
          onChange={(event) => onMode(event.target.value as ReaderMode)}
          aria-label={tr("Reading mode")}
        >
          {MODES.map((mode) => (
            <option value={mode.id} key={mode.id}>
              {tr(mode.label)}
            </option>
          ))}
        </select>
        <ChevronDown size={14} aria-hidden="true" />
      </label>
      <div className="reader-toolbar-actions">
        {!mobile && (
          <button
            type="button"
            className="reader-icon-button"
            onClick={onFullscreen}
            aria-label={tr("Fullscreen")}
            aria-pressed={fullscreen}
            title={`${tr("Fullscreen")} · F`}
          >
            <Maximize2 size={18} />
          </button>
        )}
        <button
          type="button"
          className={settingsOpen ? "reader-icon-button is-active" : "reader-icon-button"}
          onClick={onSettings}
          aria-label={tr("Reader settings")}
          aria-pressed={settingsOpen}
          title={`${tr("Reader settings")} · S`}
        >
          {settingsOpen ? <PanelRightClose size={19} /> : <PanelRightOpen size={19} />}
        </button>
      </div>
    </header>
  );
}

function PagedCanvas({
  currentPage,
  pages: pageDescriptors,
  pageUrls,
  spread,
  isRtl,
  settings,
  imageStyle,
  zoom,
  pan,
  onPrevious,
  onNext,
}: {
  currentPage: number;
  pages: PageDescriptor[];
  pageUrls: Record<number, string>;
  spread: boolean;
  isRtl: boolean;
  settings: ReaderSettings;
  imageStyle: CSSProperties;
  zoom: number;
  pan: { x: number; y: number };
  onPrevious: () => void;
  onNext: () => void;
}) {
  const previousPage = useRef(currentPage);
  const [motionDirection, setMotionDirection] = useState<
    "still" | "forward" | "backward"
  >("still");
  useEffect(() => {
    if (currentPage === previousPage.current) return;
    const numericalDirection =
      currentPage > previousPage.current ? "forward" : "backward";
    setMotionDirection(
      isRtl
        ? numericalDirection === "forward"
          ? "backward"
          : "forward"
        : numericalDirection,
    );
    previousPage.current = currentPage;
  }, [currentPage, isRtl]);
  const groups = spread ? spreadGroups(pageDescriptors) : [];
  const pages = spread
    ? (groups[spreadIndexForPage(groups, currentPage)] ?? [currentPage])
    : [currentPage];
  if (isRtl && spread) pages.reverse();
  return (
    <div
      className={[
        "paged-canvas",
        spread ? "is-spread" : "is-single",
        spread && !settings.spreadGapEnabled ? "pages-joined" : "",
        `motion-${motionDirection}`,
        `fit-${settings.fit}`,
      ].join(" ")}
      style={{ gap: spread && settings.spreadGapEnabled ? settings.gapPx : 0 }}
    >
      <button
        type="button"
        className="reader-tap-zone previous"
        aria-label={tr(isRtl ? "Next page" : "Previous page")}
        onClick={isRtl ? onNext : onPrevious}
      >
        <ArrowLeft size={22} />
      </button>
      <div
        className="page-pair"
        key={currentPage}
        style={{
          transform: `translate3d(${pan.x}px, ${pan.y}px, 0) scale(${zoom})`,
        }}
      >
        {pages.map((page) => {
          const url = pageUrls[page];
          return (
            <div className="page-frame" key={page}>
              {url !== undefined ? (
                <PageVisual
                  url={url}
                  page={page}
                  imageStyle={imageStyle}
                  widePagePolicy={settings.widePagePolicy}
                  isRtl={isRtl}
                />
              ) : (
                <PageLoading page={page} />
              )}
            </div>
          );
        })}
      </div>
      <button
        type="button"
        className="reader-tap-zone next"
        aria-label={tr(isRtl ? "Previous page" : "Next page")}
        onClick={isRtl ? onPrevious : onNext}
      >
        <ArrowLeft size={22} />
      </button>
    </div>
  );
}

function PageVisual({
  url,
  page,
  imageStyle,
  widePagePolicy,
  isRtl,
}: {
  url: string;
  page: number;
  imageStyle: CSSProperties;
  widePagePolicy: WidePagePolicy;
  isRtl: boolean;
}) {
  const [wide, setWide] = useState(false);

  useEffect(() => {
    setWide(false);
  }, [url]);

  if (wide && widePagePolicy === "split") {
    const halves = isRtl ? ["right", "left"] : ["left", "right"];
    return (
      <div
        className="split-wide-page"
        aria-label={tr("Page {{page}}, split", { page: page + 1 })}
      >
        {halves.map((side) => (
          <span className={`split-wide-half is-${side}`} key={side}>
            <img
              src={url}
              alt=""
              draggable={false}
              style={imageStyle}
            />
          </span>
        ))}
      </div>
    );
  }

  const transform =
    wide && widePagePolicy === "rotate"
      ? `${String(imageStyle.transform ?? "")} rotate(90deg)`
      : imageStyle.transform;
  return (
    <img
      className={wide && widePagePolicy === "rotate" ? "is-rotated-wide" : ""}
      src={url}
      alt={tr("Page {{page}}", { page: page + 1 })}
      draggable={false}
      style={{ ...imageStyle, transform }}
      onLoad={(event) => {
        setWide(event.currentTarget.naturalWidth > event.currentTarget.naturalHeight);
      }}
    />
  );
}

function GuidedCanvas({
  currentPage,
  pageUrl,
  step,
  isRtl,
  imageStyle,
  zoom,
  onPrevious,
  onNext,
}: {
  currentPage: number;
  pageUrl: string | undefined;
  step: number;
  isRtl: boolean;
  imageStyle: CSSProperties;
  zoom: number;
  onPrevious: () => void;
  onNext: () => void;
}) {
  const columns = isRtl ? [72, 28] : [28, 72];
  const regions = [
    [columns[0]!, 27],
    [columns[1]!, 27],
    [columns[0]!, 73],
    [columns[1]!, 73],
  ];
  const region = regions[Math.max(0, Math.min(regions.length - 1, step))]!;
  return (
    <div className="guided-canvas">
      <button
        type="button"
        className="reader-tap-zone previous"
        aria-label={tr("Previous focus region")}
        onClick={onPrevious}
      >
        <ArrowLeft size={22} />
      </button>
      <div className="guided-frame">
        {pageUrl === undefined ? (
          <PageLoading page={currentPage} />
        ) : (
          <img
            src={pageUrl}
            alt={tr("Page {{page}}, focus region {{current}} of {{total}}", {
              page: currentPage + 1,
              current: step + 1,
              total: 4,
            })}
            draggable={false}
            style={{
              ...imageStyle,
              transform: `${String(imageStyle.transform ?? "")} scale(${1.95 * zoom})`,
              transformOrigin: `${region[0]}% ${region[1]}%`,
            }}
          />
        )}
        <span className="guided-position" aria-hidden="true">
          {[0, 1, 2, 3].map((index) => (
            <i className={index === step ? "is-active" : ""} key={index} />
          ))}
        </span>
      </div>
      <button
        type="button"
        className="reader-tap-zone next"
        aria-label={tr("Next focus region")}
        onClick={onNext}
      >
        <ArrowLeft size={22} />
      </button>
    </div>
  );
}

function ContinuousPages({
  mode,
  currentPage,
  pageUrls,
  settings,
  imageStyle,
  zoom,
  onLoad,
  onVisible,
}: {
  mode: ReaderMode;
  currentPage: number;
  pageUrls: Record<number, string>;
  settings: ReaderSettings;
  imageStyle: CSSProperties;
  zoom: number;
  onLoad: (page: number) => void | Promise<void>;
  onVisible: (page: number) => void | Promise<void>;
}) {
  const manifest = useKomaStore((state) => state.reader?.payload.manifest);
  const container = useRef<HTMLDivElement>(null);
  const currentRef = useRef(currentPage);
  currentRef.current = currentPage;

  useEffect(() => {
    const root = container.current;
    if (root === null || manifest === undefined) return;
    const observer = new IntersectionObserver(
      (entries) => {
        const visible = entries
          .filter((entry) => entry.isIntersecting)
          .sort((left, right) => right.intersectionRatio - left.intersectionRatio);
        for (const entry of visible) {
          const page = Number((entry.target as HTMLElement).dataset.page);
          if (Number.isFinite(page)) void onLoad(page);
        }
        const centered = visible[0];
        if (centered !== undefined && centered.intersectionRatio >= 0.58) {
          const page = Number((centered.target as HTMLElement).dataset.page);
          if (Number.isFinite(page) && page !== currentRef.current) {
            void onVisible(page);
          }
        }
      },
      { root, rootMargin: "70% 0px", threshold: [0.08, 0.58, 0.82] },
    );
    const pages = root.querySelectorAll<HTMLElement>("[data-continuous-page]");
    pages.forEach((page) => observer.observe(page));
    const initial = root.querySelector<HTMLElement>(
      `[data-continuous-page][data-page="${currentRef.current}"]`,
    );
    requestAnimationFrame(() => {
      initial?.scrollIntoView({ block: "center" });
    });
    return () => observer.disconnect();
  }, [manifest, onLoad, onVisible]);

  if (manifest === undefined) return null;
  return (
    <div
      className={mode === "webtoon" ? "continuous-pages is-webtoon" : "continuous-pages"}
      style={{ gap: mode === "webtoon" ? 0 : settings.gapPx }}
      ref={container}
    >
      {manifest.pages.map((descriptor) => {
        const url = pageUrls[descriptor.index];
        const ratio =
          descriptor.width !== null && descriptor.height !== null
            ? descriptor.width / descriptor.height
            : 2 / 3;
        return (
          <div
            className="continuous-page"
            data-continuous-page
            data-page={descriptor.index}
            style={{
              aspectRatio: ratio,
              width: `${Math.round(zoom * 100)}%`,
            }}
            key={descriptor.index}
          >
            {url !== undefined ? (
              <img
                src={url}
                alt={tr("Page {{page}}", { page: descriptor.index + 1 })}
                loading="lazy"
                draggable={false}
                style={imageStyle}
              />
            ) : (
              <PageLoading page={descriptor.index} />
            )}
          </div>
        );
      })}
    </div>
  );
}

function PageLoading({ page }: { page: number }) {
  return (
    <div
      className="page-loading"
      aria-label={tr("Loading page {{page}}", { page: page + 1 })}
    >
      <span />
      <small>{page + 1}</small>
    </div>
  );
}

function ReaderScrubber({
  currentPage,
  pageCount,
  isRtl,
  bookmarked,
  controlsVisible,
  onPage,
  onBookmark,
}: {
  currentPage: number;
  pageCount: number;
  isRtl: boolean;
  bookmarked: boolean;
  controlsVisible: boolean;
  onPage: (page: number) => void;
  onBookmark: () => void;
}) {
  const [draftPage, setDraftPage] = useState(currentPage + 1);
  const [scrubbing, setScrubbing] = useState(false);
  useEffect(() => {
    if (!scrubbing) setDraftPage(currentPage + 1);
  }, [currentPage, scrubbing]);

  const commitPage = (value: number[]) => {
    const page = Math.max(1, Math.min(pageCount, value[0] ?? 1));
    setDraftPage(page);
    setScrubbing(false);
    onPage(page - 1);
  };

  return (
    <footer
      className="reader-scrubber"
      aria-hidden={!controlsVisible}
      inert={!controlsVisible}
    >
      <span className="scrubber-page">{draftPage}</span>
      <Slider.Root
        className="reader-page-slider"
        min={1}
        max={Math.max(1, pageCount)}
        step={1}
        value={[draftPage]}
        onValueChange={(value) => {
          setScrubbing(true);
          setDraftPage(value[0] ?? 1);
        }}
        onValueCommit={commitPage}
        onPointerCancel={() => {
          setScrubbing(false);
          setDraftPage(currentPage + 1);
        }}
        aria-label={tr("Page")}
        dir={isRtl ? "rtl" : "ltr"}
      >
        <Slider.Track className="reader-page-track">
          <Slider.Range className="reader-page-range" />
        </Slider.Track>
        <Slider.Thumb className="reader-page-thumb" aria-label={tr("Page")} />
      </Slider.Root>
      <span className="scrubber-total">{pageCount}</span>
      <button
        type="button"
        className={bookmarked ? "reader-icon-button is-active" : "reader-icon-button"}
        onClick={onBookmark}
        aria-label={tr(bookmarked ? "Remove bookmark" : "Bookmark this page")}
        aria-pressed={bookmarked}
        title={`${tr("Bookmark")} · B`}
      >
        <Bookmark size={18} fill={bookmarked ? "currentColor" : "none"} />
      </button>
    </footer>
  );
}

function ReaderSettingsPanel({
  open,
  settings,
  zoom,
  currentPage,
  bookmarks,
  metadataDirection,
  onChange,
  onZoom,
  onBookmarkPage,
  onRemoveBookmark,
  onSaveAnnotation,
  onClose,
}: {
  open: boolean;
  settings: ReaderSettings;
  zoom: number;
  currentPage: number;
  bookmarks: BookmarkRecord[];
  metadataDirection: ReadingDirection;
  onChange: (patch: Partial<ReaderSettings>) => void;
  onZoom: (zoom: number) => void;
  onBookmarkPage: (page: number) => void;
  onRemoveBookmark: (id: string) => void;
  onSaveAnnotation: (label: string, note: string) => void;
  onClose: () => void;
}) {
  return (
    <aside
      className={`reader-settings${open ? " is-open" : ""}`}
      aria-hidden={!open}
      inert={!open}
    >
      <div className="reader-settings-header">
        <div>
          <h2>{tr("Reader settings")}</h2>
        </div>
        <button
          type="button"
          className="reader-icon-button"
          onClick={onClose}
          aria-label={tr("Close reader settings")}
        >
          <X size={18} />
        </button>
      </div>
      <div className="reader-settings-scroll">
        <ReaderSettingGroup title={tr("Layout")}>
          <label className="reader-field-label">{tr("Reading mode")}</label>
          <div className="reader-choice-grid">
            {MODES.filter((mode) => mode.id !== "presentation").map((mode) => {
              const Icon = mode.icon;
              return (
                <button
                  type="button"
                  className={settings.mode === mode.id ? "is-active" : ""}
                  onClick={() => onChange({ mode: mode.id })}
                  aria-pressed={settings.mode === mode.id}
                  key={mode.id}
                >
                  <Icon size={17} />
                  {tr(mode.label)}
                </button>
              );
            })}
          </div>
          {settings.mode === "spreads" && (
            <>
              <ReaderSwitch
                label={tr("Page gap")}
                checked={settings.spreadGapEnabled}
                onChange={(spreadGapEnabled) => onChange({ spreadGapEnabled })}
              />
              <label className="image-slider">
                <span>{tr("Gap size")}</span>
                <Slider.Root
                  className="slider-root"
                  min={0}
                  max={64}
                  step={1}
                  value={[settings.gapPx]}
                  disabled={!settings.spreadGapEnabled}
                  onValueChange={(value) => onChange({ gapPx: value[0] ?? 0 })}
                  aria-label={tr("Page gap")}
                >
                  <Slider.Track className="slider-track">
                    <Slider.Range className="slider-range" />
                  </Slider.Track>
                  <Slider.Thumb className="slider-thumb" />
                </Slider.Root>
                <output>{settings.spreadGapEnabled ? `${settings.gapPx}px` : tr("Off")}</output>
              </label>
            </>
          )}
          <label className="reader-field-label">{tr("Direction")}</label>
          <div className="reader-segmented">
            {(
              [
                ["automatic", `${tr("Automatic")}${metadataDirection !== "automatic" ? ` · ${metadataDirection === "rightToLeft" ? "RTL" : "LTR"}` : ""}`],
                ["leftToRight", tr("Left to right")],
                ["rightToLeft", tr("Right to left")],
              ] as Array<[ReadingDirection, string]>
            ).map(([value, label]) => (
              <button
                type="button"
                className={settings.direction === value ? "is-active" : ""}
                aria-pressed={settings.direction === value}
                onClick={() => onChange({ direction: value })}
                key={value}
              >
                {label}
              </button>
            ))}
          </div>
        </ReaderSettingGroup>

        <ReaderSettingGroup title={tr("Page")}>
          <label className="reader-field-label">{tr("Fit")}</label>
          <div className="reader-segmented">
            {(
              [
                ["smart", tr("Smart")],
                ["page", tr("Page")],
                ["width", tr("Width")],
                ["height", tr("Height")],
              ] as Array<[FitMode, string]>
            ).map(([value, label]) => (
              <button
                type="button"
                className={settings.fit === value ? "is-active" : ""}
                onClick={() => onChange({ fit: value })}
                aria-pressed={settings.fit === value}
                key={value}
              >
                {label}
              </button>
            ))}
          </div>
          <div className="zoom-control">
            <button
              type="button"
              className="reader-icon-button"
              onClick={() => onZoom(zoom / 1.15)}
              aria-label={tr("Zoom out")}
            >
              <ZoomOut size={17} />
            </button>
            <Slider.Root
              className="slider-root"
              min={25}
              max={500}
              step={5}
              value={[Math.round(zoom * 100)]}
              onValueChange={(value) => onZoom((value[0] ?? 100) / 100)}
              aria-label={tr("Zoom")}
            >
              <Slider.Track className="slider-track">
                <Slider.Range className="slider-range" />
              </Slider.Track>
              <Slider.Thumb className="slider-thumb" aria-label={tr("Zoom")} />
            </Slider.Root>
            <button
              type="button"
              className="reader-icon-button"
              onClick={() => onZoom(zoom * 1.15)}
              aria-label={tr("Zoom in")}
            >
              <ZoomIn size={17} />
            </button>
            <span>{Math.round(zoom * 100)}%</span>
            <button
              type="button"
              className="reader-icon-button"
              onClick={() => onZoom(1)}
              aria-label={tr("Reset zoom")}
              title={tr("Reset zoom")}
            >
              <RotateCcw size={16} />
            </button>
          </div>
          <div className="zoom-presets" aria-label={tr("Zoom presets")}>
            {[1, 1.5, 2, 3].map((value) => (
              <button
                type="button"
                className={Math.abs(zoom - value) < 0.01 ? "is-active" : ""}
                onClick={() => onZoom(value)}
                aria-pressed={Math.abs(zoom - value) < 0.01}
                key={value}
              >
                {Math.round(value * 100)}%
              </button>
            ))}
          </div>
          <ReaderSwitch
            label={tr("Trim margins")}
            checked={settings.cropMargins}
            onChange={(cropMargins) => onChange({ cropMargins })}
          />
          <ReaderSwitch
            label={tr("Page number")}
            checked={settings.showPageNumber}
            onChange={(showPageNumber) => onChange({ showPageNumber })}
          />
          <label className="reader-field-label">{tr("Wide pages")}</label>
          <div className="reader-segmented">
            {(
              [
                ["keep", tr("Keep")],
                ["split", tr("Split")],
                ["rotate", tr("Rotate")],
              ] as Array<[WidePagePolicy, string]>
            ).map(([value, label]) => (
              <button
                type="button"
                className={settings.widePagePolicy === value ? "is-active" : ""}
                onClick={() => onChange({ widePagePolicy: value })}
                aria-pressed={settings.widePagePolicy === value}
                key={value}
              >
                {label}
              </button>
            ))}
          </div>
          <ReaderSwitch
            label={tr("Keep screen awake")}
            checked={settings.keepAwake}
            onChange={(keepAwake) => onChange({ keepAwake })}
          />
        </ReaderSettingGroup>

        <ReaderSettingGroup title={tr("Image")}>
          <ImageSlider
            label={tr("Brightness")}
            icon={<Sun size={15} />}
            value={settings.brightness}
            onChange={(brightness) => onChange({ brightness })}
          />
          <ImageSlider
            label={tr("Contrast")}
            icon={<SlidersHorizontal size={15} />}
            value={settings.contrast}
            onChange={(contrast) => onChange({ contrast })}
          />
          <ImageSlider
            label={tr("Saturation")}
            icon={<Moon size={15} />}
            value={settings.saturation}
            onChange={(saturation) => onChange({ saturation })}
          />
          <ImageSlider
            label={tr("Gamma")}
            icon={<SlidersHorizontal size={15} />}
            value={settings.gamma}
            onChange={(gamma) => onChange({ gamma })}
          />
          <ReaderSwitch
            label={tr("Grayscale")}
            checked={settings.grayscale}
            onChange={(grayscale) => onChange({ grayscale })}
          />
          <ReaderSwitch
            label={tr("Invert")}
            checked={settings.invert}
            onChange={(invert) => onChange({ invert })}
          />
          <ReaderSwitch
            label={tr("Sharpen")}
            checked={settings.sharpen}
            onChange={(sharpen) => onChange({ sharpen })}
          />
        </ReaderSettingGroup>

        <ReaderSettingGroup title={tr("Page note")}>
          <ReaderAnnotationEditor
            currentPage={currentPage}
            bookmark={bookmarks.find(
              (bookmark) => bookmark.pageIndex === currentPage,
            )}
            onSave={onSaveAnnotation}
          />
        </ReaderSettingGroup>

        <ReaderSettingGroup title={tr("Bookmarks & notes")}>
          {bookmarks.length === 0 ? (
            <p className="reader-bookmark-empty">
              {tr("No bookmarks.")}
            </p>
          ) : (
            <div className="reader-bookmark-list">
              {bookmarks.map((bookmark) => (
                <div key={bookmark.id}>
                  <button
                    type="button"
                    onClick={() => onBookmarkPage(bookmark.pageIndex)}
                  >
                    <Bookmark size={14} fill="currentColor" />
                    <span>
                      <strong>
                        {bookmark.label ??
                          tr("Page {{page}}", {
                            page: bookmark.pageIndex + 1,
                          })}
                      </strong>
                      <small>
                        {bookmark.note ??
                          tr("Page {{page}}", {
                            page: bookmark.pageIndex + 1,
                          })}
                      </small>
                    </span>
                  </button>
                  <button
                    type="button"
                    className="reader-icon-button"
                    onClick={() => onRemoveBookmark(bookmark.id)}
                    aria-label={tr("Remove bookmark for page {{page}}", {
                      page: bookmark.pageIndex + 1,
                    })}
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              ))}
            </div>
          )}
        </ReaderSettingGroup>
      </div>
    </aside>
  );
}

function ReaderAnnotationEditor({
  currentPage,
  bookmark,
  onSave,
}: {
  currentPage: number;
  bookmark: BookmarkRecord | undefined;
  onSave: (label: string, note: string) => void;
}) {
  const [label, setLabel] = useState(bookmark?.label ?? "");
  const [note, setNote] = useState(bookmark?.note ?? "");

  useEffect(() => {
    setLabel(bookmark?.label ?? "");
    setNote(bookmark?.note ?? "");
  }, [bookmark?.id, bookmark?.label, bookmark?.note, currentPage]);

  return (
    <div className="reader-annotation-editor">
      <label>
        <span>{tr("Label")}</span>
        <input
          type="text"
          value={label}
          maxLength={512}
          placeholder={tr("Page {{page}}", { page: currentPage + 1 })}
          onChange={(event) => setLabel(event.target.value)}
        />
      </label>
      <label>
        <span>{tr("Note")}</span>
        <textarea
          value={note}
          maxLength={64 * 1024}
          rows={4}
          placeholder={tr("Add a note")}
          onChange={(event) => setNote(event.target.value)}
        />
      </label>
      <button
        type="button"
        className="reader-note-save"
        disabled={label.trim().length === 0 && note.trim().length === 0}
        onClick={() => onSave(label.trim(), note.trim())}
      >
        {tr(bookmark === undefined ? "Save note" : "Update note")}
      </button>
    </div>
  );
}

function ReaderSettingGroup({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  return (
    <section className="reader-setting-group">
      <h3>{title}</h3>
      {children}
    </section>
  );
}

function ReaderSwitch({
  label,
  detail,
  checked,
  onChange,
}: {
  label: string;
  detail?: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="reader-switch">
      <span>
        <strong>{label}</strong>
        {detail !== undefined && <small>{detail}</small>}
      </span>
      <input
        type="checkbox"
        checked={checked}
        onChange={(event) => onChange(event.target.checked)}
      />
      <span className="switch-track">
        <span />
      </span>
    </label>
  );
}

function ImageSlider({
  label,
  icon,
  value,
  onChange,
}: {
  label: string;
  icon: ReactNode;
  value: number;
  onChange: (value: number) => void;
}) {
  return (
    <label className="image-slider">
      <span>
        {icon}
        {label}
      </span>
      <Slider.Root
        className="slider-root"
        min={50}
        max={150}
        step={5}
        value={[Math.round(value * 100)]}
        onValueChange={(next) => onChange((next[0] ?? 100) / 100)}
      >
        <Slider.Track className="slider-track">
          <Slider.Range className="slider-range" />
        </Slider.Track>
        <Slider.Thumb className="slider-thumb" aria-label={label} />
      </Slider.Root>
      <output>{Math.round(value * 100)}%</output>
    </label>
  );
}

function isReaderControl(target: EventTarget | null): boolean {
  return (
    target instanceof Element &&
    target.closest(
      "button, input, textarea, select, [role=menu], [role=slider], .slider-root, .reader-page-slider, .reader-mode-button",
    ) !== null
  );
}

async function setReaderFullscreen(fullscreen: boolean): Promise<void> {
  if ("__TAURI_INTERNALS__" in window) {
    await getCurrentWindow().setFullscreen(fullscreen);
    return;
  }
  if (fullscreen && document.fullscreenElement === null) {
    await document.documentElement.requestFullscreen();
  } else if (!fullscreen && document.fullscreenElement !== null) {
    await document.exitFullscreen();
  }
}

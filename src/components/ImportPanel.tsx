import {
  Check,
  Clipboard,
  Download,
  FolderOpen,
  LoaderCircle,
  LockKeyhole,
  ShieldAlert,
  ShieldCheck,
  X,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { locale, tr } from "../i18n";
import { backend, errorCode, errorMessage } from "../lib/backend";
import { useKomaStore } from "../store/koma";
import type { ImportEvent, ImportPreview } from "../types";

type ImportPhase =
  | "idle"
  | "checking"
  | "ready"
  | "downloading"
  | "packaging"
  | "complete"
  | "error";

interface ProgressState {
  completed: number;
  total: number;
  label: string;
}

function languageName(language: string): string {
  const displayCode = language.toLowerCase() === "es-la" ? "es-419" : language;
  try {
    return (
      new Intl.DisplayNames(locale(), { type: "language" }).of(displayCode) ??
      language.toUpperCase()
    );
  } catch {
    return language.toUpperCase();
  }
}

export function ImportPanel() {
  const open = useKomaStore((state) => state.importOpen);
  const setOpen = useKomaStore((state) => state.setImportOpen);
  const bootstrap = useKomaStore((state) => state.bootstrap);
  const addImportedItem = useKomaStore((state) => state.addImportedItem);
  const notify = useKomaStore((state) => state.notify);
  const [source, setSource] = useState("");
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [selectedVolume, setSelectedVolume] = useState<number | null>(null);
  const [selectedChapter, setSelectedChapter] = useState<number | null>(null);
  const [scope, setScope] = useState<"chapter" | "volume" | "series">("volume");
  const [destination, setDestination] = useState(
    bootstrap?.defaultImportDirectory ?? "~/Downloads/Koma",
  );
  const [confirmed, setConfirmed] = useState(false);
  const [phase, setPhase] = useState<ImportPhase>("idle");
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<ProgressState>({
    completed: 0,
    total: 0,
    label: "",
  });
  const mobile =
    bootstrap?.platform === "ios" || bootstrap?.platform === "android";

  useEffect(() => {
    if (bootstrap !== null && destination === "~/Downloads/Koma") {
      setDestination(bootstrap.defaultImportDirectory);
    }
  }, [bootstrap, destination]);

  useEffect(() => {
    let active = true;
    let removeListener: (() => void) | null = null;
    void backend.onImportEvent((event: ImportEvent) => {
      if (!active) return;
      if (event.kind === "checking") {
        setPhase("checking");
        setProgress({ completed: 0, total: 0, label: tr("Checking link…") });
      } else if (event.kind === "eligible") {
        setProgress({ completed: 0, total: 0, label: tr("Link ready") });
      } else if (event.kind === "discovered") {
        setProgress({
          completed: 0,
          total: event.pageCount,
          label: tr("Preparing {{count}} pages", { count: event.pageCount }),
        });
      } else if (event.kind === "downloading") {
        setPhase("downloading");
        setProgress({
          completed: event.completed,
          total: event.total,
          label: tr("Downloading page {{current}} of {{total}}", {
            current: event.completed,
            total: event.total,
          }),
        });
      } else if (event.kind === "recovering") {
        setPhase("downloading");
        setProgress((current) => ({
          ...current,
          label: tr("Recovering {{count}} pages", {
            count: event.failedPages,
          }),
        }));
      } else if (event.kind === "packaging") {
        setPhase("packaging");
        setProgress((current) => ({
          completed: current.total,
          total: current.total,
          label: tr("Packaging CBZ…"),
        }));
      } else {
        setPhase("complete");
        setProgress({
          completed: event.receipt.pageCount,
          total: event.receipt.pageCount,
          label: tr("Ready"),
        });
      }
    }).then((unlisten) => {
      if (active) removeListener = unlisten;
      else unlisten();
    });
    return () => {
      active = false;
      removeListener?.();
    };
  }, []);

  const percent = useMemo(() => {
    if (progress.total <= 0) return 0;
    return Math.round((progress.completed / progress.total) * 100);
  }, [progress]);

  const resetPreview = (value: string) => {
    setSource(value);
    setPreview(null);
    setSelectedVolume(null);
    setSelectedChapter(null);
    setScope("volume");
    setConfirmed(false);
    setPhase("idle");
    setError(null);
    setProgress({ completed: 0, total: 0, label: "" });
  };

  const verify = async () => {
    const trimmed = source.trim();
    if (trimmed.length === 0) {
      setError(tr("Paste a source link."));
      setPhase("error");
      return;
    }
    setPhase("checking");
    setError(null);
    setPreview(null);
    setProgress({ completed: 0, total: 0, label: tr("Checking link…") });
    try {
      const result = await backend.previewLink(trimmed);
      setPreview(result);
      setSelectedVolume(result.selectedVolumeId);
      setSelectedChapter(result.selectedChapterId);
      setScope(
        result.availableScopes.includes("volume") ? "volume" : "series",
      );
      setPhase("ready");
      setProgress({
        completed: 0,
        total: result.estimatedPageCount ?? 0,
        label: tr("Ready"),
      });
    } catch (caught) {
      const code = errorCode(caught);
      setPhase("error");
      setError(
        code === "import_denied"
          ? tr("This link is not available for import.")
          : errorMessage(caught),
      );
    }
  };

  const changeDestination = async () => {
    const selected = await backend.pickFolder();
    if (selected !== null) {
      setDestination(selected);
    } else if (backend.kind === "preview") {
      notify(tr("Destination picker"), tr("Available in the desktop app."));
    }
  };

  const paste = async () => {
    try {
      resetPreview(await navigator.clipboard.readText());
    } catch {
      notify(
        tr("Clipboard unavailable"),
        tr("Paste into the link field."),
        "warning",
      );
    }
  };

  const runImport = async () => {
    if (
      preview === null ||
      (scope === "volume" && selectedVolume === null) ||
      (scope === "chapter" && selectedChapter === null) ||
      !confirmed
    ) return;
    setPhase("checking");
    setError(null);
    const options = {
      destinationDirectory: destination,
      volumeId: selectedVolume,
      chapterId: selectedChapter,
      scope,
      preferredLanguage:
        (scope === "chapter"
          ? preview.chapters.find((chapter) => chapter.id === selectedChapter)
              ?.language
          : preview.volumes.find((volume) => volume.id === selectedVolume)
              ?.language) ?? "en",
      overwriteExisting: false,
      downloadConcurrency: 6,
    };
    let retry = 0;
    for (;;) {
      try {
        const result = await backend.importLink(source.trim(), options);
        addImportedItem(result.item);
        setPhase("complete");
        notify(
          scope === "chapter"
            ? tr("Chapter imported")
            : scope === "series"
              ? tr("Series imported")
              : tr("Volume imported"),
          tr("{{count}} pages saved as CBZ.", {
            count: result.receipt.pageCount,
          }),
          "success",
        );
        return;
      } catch (caught) {
        const code = errorCode(caught);
        const message = errorMessage(caught);
        const networkFailure =
          code === "provider_unavailable" ||
          message.toLowerCase().includes("network error") ||
          message.toLowerCase().includes("timed out");
        if (!networkFailure || retry >= 11) {
          setPhase("error");
          setError(
            networkFailure
              ? tr("Download paused. Check your connection, then try again to continue.")
              : message,
          );
          if (!useKomaStore.getState().importOpen) {
            notify(tr("Download paused"), message, "warning");
          }
          return;
        }
        retry += 1;
        setPhase("downloading");
        setProgress((current) => ({
          ...current,
          label: tr("Connection lost. Retrying…"),
        }));
        await new Promise((resolve) =>
          window.setTimeout(resolve, Math.min(30_000, retry * 5_000)),
        );
      }
    }
  };

  const close = () => {
    if (phase === "downloading" || phase === "packaging") {
      notify(
        tr("Download continues in the background"),
        tr("Koma will notify you when it is ready."),
      );
    }
    setOpen(false);
  };

  return (
    <>
      {open && (
        <button
          type="button"
          className="drawer-scrim"
          aria-label={tr("Close importer")}
          onClick={close}
        />
      )}
      <aside
        className={`import-panel${open ? " is-open" : ""}`}
        aria-label={tr("Import from link")}
        aria-hidden={!open}
      >
        <div className="drawer-header">
          <div>
            <h2>{tr("Import from link")}</h2>
          </div>
          <button
            type="button"
            className="icon-button"
            aria-label={tr("Close importer")}
            onClick={close}
          >
            <X size={19} />
          </button>
        </div>

        <div className="import-body">
          <div className="permission-notice">
            <ShieldAlert size={20} aria-hidden="true" />
            <div>
              <strong>{tr("Properly released works only.")}</strong>
              <p>
                {bootstrap?.importWarning ??
                  tr("Only import work you own or have permission to download.")}
              </p>
            </div>
          </div>

          <label className="field-label" htmlFor="import-source">
            {tr("Source link")}
          </label>
          <div className="link-field">
            <input
              id="import-source"
              type="url"
              value={source}
              onChange={(event) => resetPreview(event.target.value)}
              placeholder="https://…"
              autoComplete="off"
              spellCheck={false}
              disabled={phase === "downloading" || phase === "packaging"}
            />
            <button
              type="button"
              className="icon-button"
              aria-label={tr("Paste from clipboard")}
              title={tr("Paste from clipboard")}
              onClick={() => void paste()}
            >
              <Clipboard size={17} />
            </button>
          </div>
          {preview === null && phase !== "error" && (
            <button
              type="button"
              className="primary-button wide"
              onClick={() => void verify()}
              disabled={phase === "checking" || source.trim().length === 0}
            >
              {phase === "checking" ? (
                <LoaderCircle className="spin" size={17} />
              ) : (
                <ShieldCheck size={17} />
              )}
              {phase === "checking" ? tr("Checking…") : tr("Check link")}
            </button>
          )}

          {error !== null && (
            <div className="import-error" role="alert">
              <LockKeyhole size={18} />
              <div>
                <strong>{tr("Import stopped")}</strong>
                <p>{error}</p>
              </div>
            </div>
          )}

          {preview !== null && (
            <>
              <div className="approval-result">
                <span className="approval-icon">
                  <Check size={16} />
                </span>
                <div>
                  <strong>{preview.provider}</strong>
                  <p>{preview.title}</p>
                </div>
              </div>

              <div className="import-scope segmented" aria-label={tr("Import")}>
                {preview.availableScopes.includes("chapter") && (
                  <button
                    type="button"
                    className={scope === "chapter" ? "is-active" : ""}
                    aria-pressed={scope === "chapter"}
                    onClick={() => setScope("chapter")}
                  >
                    {tr("Chapter")}
                  </button>
                )}
                {preview.availableScopes.includes("volume") && (
                  <button
                    type="button"
                    className={scope === "volume" ? "is-active" : ""}
                    aria-pressed={scope === "volume"}
                    onClick={() => setScope("volume")}
                  >
                    {tr("Volume")}
                  </button>
                )}
                {preview.availableScopes.includes("series") && (
                  <button
                    type="button"
                    className={scope === "series" ? "is-active" : ""}
                    aria-pressed={scope === "series"}
                    onClick={() => setScope("series")}
                  >
                    {tr("Entire series")}
                  </button>
                )}
              </div>

              {scope === "series" && (
                <div className="series-import-summary">
                  <strong>{tr("Earliest to latest")}</strong>
                  <span>
                    {tr("{{count}} chapters", {
                      count: preview.seriesChapterCount ?? 0,
                    })}
                  </span>
                  <small>
                    {preview.seriesPageCount === null
                      ? tr("Page count checked before download")
                      : tr("{{count}} pages", {
                          count: preview.seriesPageCount,
                        })}
                  </small>
                </div>
              )}

              {scope === "chapter" && (
                <div className="chapter-picker">
                  <label className="field-label" htmlFor="import-chapter">
                    {tr("Choose chapter")}
                  </label>
                  <select
                    id="import-chapter"
                    value={selectedChapter ?? ""}
                    onChange={(event) =>
                      setSelectedChapter(Number(event.target.value))
                    }
                    disabled={phase === "downloading" || phase === "packaging"}
                  >
                    {[...preview.chapters].reverse().map((chapter) => (
                      <option value={chapter.id} key={chapter.id}>
                        {tr("Chapter {{number}}", {
                          number: chapter.number,
                        })}
                        {chapter.name === null ? "" : ` · ${chapter.name}`}
                      </option>
                    ))}
                  </select>
                  <span>
                    {languageName(
                      preview.chapters.find(
                        (chapter) => chapter.id === selectedChapter,
                      )?.language ?? "en",
                    )}
                    {" · "}
                    {preview.chapters.find(
                      (chapter) => chapter.id === selectedChapter,
                    )?.pageCount === null
                      ? tr("Page count checked before download")
                      : tr("{{count}} pages", {
                          count:
                            preview.chapters.find(
                              (chapter) => chapter.id === selectedChapter,
                            )?.pageCount ?? 0,
                        })}
                  </span>
                </div>
              )}

              {scope === "volume" && <fieldset className="volume-picker">
                <legend>{tr("Choose volume")}</legend>
                {preview.volumes.map((volume) => (
                  <label
                    className={
                      selectedVolume === volume.id
                        ? "volume-option is-selected"
                        : "volume-option"
                    }
                    key={volume.id}
                  >
                    <input
                      type="radio"
                      name="import-volume"
                      value={volume.id}
                      checked={selectedVolume === volume.id}
                      onChange={() => setSelectedVolume(volume.id)}
                      disabled={phase === "downloading" || phase === "packaging"}
                    />
                    <span className="radio-mark" />
                    <span>
                      <strong>
                        {tr("Volume")} {volume.number} · {languageName(volume.language)}
                      </strong>
                      <small>
                        {volume.pageCount !== null
                          ? tr("{{count}} pages", { count: volume.pageCount })
                          : volume.chapterCount !== null
                            ? tr("{{count}} chapters", { count: volume.chapterCount })
                            : tr("Page count checked before download")}
                      </small>
                    </span>
                  </label>
                ))}
              </fieldset>}

              {!mobile && <div className="destination-row">
                <div>
                  <span>{tr("Save to")}</span>
                  <strong title={destination}>{destination}</strong>
                </div>
                <button
                  type="button"
                  className="icon-button"
                  aria-label={tr("Choose destination")}
                  onClick={() => void changeDestination()}
                  disabled={phase === "downloading" || phase === "packaging"}
                >
                  <FolderOpen size={17} />
                </button>
              </div>}

              <label className="confirmation-row">
                <input
                  type="checkbox"
                  checked={confirmed}
                  onChange={(event) => setConfirmed(event.target.checked)}
                  disabled={phase === "downloading" || phase === "packaging"}
                />
                <span className="checkbox-mark">
                  <Check size={13} />
                </span>
                <span>
                  {tr("I have permission to download this work.")}
                </span>
              </label>

              {(phase === "downloading" ||
                phase === "packaging" ||
                phase === "complete") && (
                <div className="import-progress" aria-live="polite">
                  <div>
                    <span>{progress.label}</span>
                    <strong>{phase === "complete" ? tr("Done") : `${percent}%`}</strong>
                  </div>
                  <div className="progress-track">
                    <span
                      style={{
                        width:
                          phase === "complete"
                            ? "100%"
                            : `${Math.max(2, percent)}%`,
                      }}
                    />
                  </div>
                </div>
              )}

              {phase === "complete" ? (
                <button
                  type="button"
                  className="primary-button wide"
                  onClick={() => setOpen(false)}
                >
                  <BookReadyIcon />
                  {tr("Open library")}
                </button>
              ) : (
                <button
                  type="button"
                  className="primary-button wide"
                  onClick={() => void runImport()}
                  disabled={
                    !confirmed ||
                    (scope === "volume" && selectedVolume === null) ||
                    (scope === "chapter" && selectedChapter === null) ||
                    phase === "downloading" ||
                    phase === "packaging" ||
                    phase === "checking"
                  }
                >
                  {phase === "downloading" || phase === "packaging" ? (
                    <LoaderCircle className="spin" size={17} />
                  ) : (
                    <Download size={17} />
                  )}
                  {phase === "downloading"
                    ? tr("Downloading…")
                    : phase === "packaging"
                      ? tr("Packaging CBZ…")
                      : tr("Download and add to Koma")}
                </button>
              )}
            </>
          )}
        </div>
      </aside>
    </>
  );
}

function BookReadyIcon() {
  return (
    <span className="book-ready-icon" aria-hidden="true">
      <span />
      <span />
    </span>
  );
}

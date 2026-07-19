import * as Slider from "@radix-ui/react-slider";
import {
  AlertTriangle,
  CheckCircle2,
  FileCheck2,
  FileCog,
  Info,
  LoaderCircle,
  RefreshCw,
  Save,
  ShieldCheck,
  Wrench,
  X,
} from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";

import { locale, tr } from "../i18n";
import { backend, errorCode, errorMessage } from "../lib/backend";
import { useKomaStore } from "../store/koma";
import type {
  ConversionOptions,
  ConversionReport,
  InspectionIssue,
  OutputImageFormat,
  PublicationInspection,
  PublicationMetadata,
} from "../types";

type ToolTab = "health" | "metadata" | "convert";

const DEFAULT_CONVERSION: ConversionOptions = {
  imageFormat: "original",
  jpegQuality: 90,
  maxDimension: null,
  skipUnreadablePages: false,
};

export function ToolsPanel() {
  const itemId = useKomaStore((state) => state.toolsItemId);
  const item = useKomaStore((state) =>
    state.items.find((candidate) => candidate.id === itemId),
  );
  const setItemId = useKomaStore((state) => state.setToolsItemId);
  const addImportedItem = useKomaStore((state) => state.addImportedItem);
  const requestPassword = useKomaStore((state) => state.requestPassword);
  const notify = useKomaStore((state) => state.notify);
  const bootstrap = useKomaStore((state) => state.bootstrap);
  const [tab, setTab] = useState<ToolTab>("health");
  const [inspection, setInspection] = useState<PublicationInspection | null>(null);
  const [metadata, setMetadata] = useState<PublicationMetadata | null>(null);
  const [conversion, setConversion] =
    useState<ConversionOptions>(DEFAULT_CONVERSION);
  const [writeToSource, setWriteToSource] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [lastReport, setLastReport] = useState<ConversionReport | null>(null);

  const inspect = async () => {
    if (item === undefined) return;
    setBusy("Inspecting pages…");
    setError(null);
    let password: string | undefined;
    try {
      for (;;) {
        try {
          const result = await backend.inspectPublication(item.id, password);
          setInspection(result);
          setMetadata(result.metadata);
          break;
        } catch (caught) {
          if (errorCode(caught) !== "password_required") throw caught;
          const entered = await requestPassword(item.title);
          if (entered === null) throw new Error("Inspection cancelled");
          password = entered;
        }
      }
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setBusy(null);
    }
  };

  useEffect(() => {
    if (item === undefined) return;
    setTab("health");
    setInspection(null);
    setMetadata(null);
    setConversion(DEFAULT_CONVERSION);
    setWriteToSource(false);
    setError(null);
    setLastReport(null);
    void inspect();
    // The publication identity is the reset boundary for the workbench.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [item?.id]);

  if (item === undefined) return null;

  const close = () => {
    if (busy === null) setItemId(null);
  };
  const canWriteSource = item.format === "cbz" || item.format === "folder";
  const issueCounts = inspection?.issues.reduce(
    (counts, issue) => {
      counts[issue.severity] += 1;
      return counts;
    },
    { information: 0, warning: 0, error: 0 },
  );

  const runConversion = async (repair: boolean) => {
    const suffix = repair ? tr("repaired") : tr("converted");
    const fileName = `${safeFileName(item.title)} (${suffix}).cbz`;
    const mobile =
      bootstrap?.platform === "ios" || bootstrap?.platform === "android";
    const destination = mobile
      ? `${bootstrap.defaultImportDirectory}/${fileName}`
      : await backend.pickCbzDestination(fileName);
    if (destination === null) return;
    setBusy(repair ? "Repairing publication…" : "Converting publication…");
    setError(null);
    try {
      const result = repair
        ? await backend.repairPublication(item.id, destination)
        : await backend.convertPublication(item.id, destination, conversion);
      addImportedItem(result.item);
      setLastReport(result.report);
      notify(
        tr(repair ? "Repaired CBZ created" : "Converted CBZ created"),
        `${tr("{{count}} pages", {
          count: result.report.pageCount,
        })} · ${formatBytes(result.report.outputBytes)}`,
        result.report.skippedPages.length > 0 ? "warning" : "success",
      );
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setBusy(null);
    }
  };

  const saveMetadata = async () => {
    if (metadata === null) return;
    if (metadata.title.trim().length === 0) {
      setError(tr("A publication title cannot be empty."));
      return;
    }
    setBusy(writeToSource ? "Writing metadata…" : "Saving metadata…");
    setError(null);
    try {
      const result = await backend.saveMetadata(
        item.id,
        metadata,
        writeToSource && canWriteSource,
      );
      addImportedItem(result.item);
      setInspection((current) =>
        current === null ? current : { ...current, metadata },
      );
      notify(
        tr("Metadata saved"),
        result.backupPath === null
          ? undefined
          : tr("Backup: {{path}}", { path: result.backupPath }),
        "success",
      );
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setBusy(null);
    }
  };

  return (
    <>
      <button
        type="button"
        className="drawer-scrim"
        aria-label={tr("Close publication tools")}
        onClick={close}
      />
      <aside
        className="tools-panel"
        role="dialog"
        aria-modal="true"
        aria-label={tr("Publication tools for {{title}}", { title: item.title })}
      >
        <header className="drawer-header tools-header">
          <div>
            <span className="eyebrow">{tr("Publication tools")}</span>
            <h2>{item.title}</h2>
          </div>
          <button
            type="button"
            className="icon-button"
            aria-label={tr("Close publication tools")}
            onClick={close}
            disabled={busy !== null}
          >
            <X size={18} />
          </button>
        </header>

        <div
          className="tools-tabs"
          role="tablist"
          aria-label={tr("Publication tools")}
        >
          {(
            [
              ["health", "Health", FileCheck2],
              ["metadata", "Metadata", FileCog],
              ["convert", "Convert & repair", Wrench],
            ] as const
          ).map(([id, label, Icon]) => (
            <button
              type="button"
              role="tab"
              aria-selected={tab === id}
              className={tab === id ? "is-active" : ""}
              onClick={() => setTab(id)}
              key={id}
            >
              <Icon size={15} />
              {tr(label)}
            </button>
          ))}
        </div>

        <div className="tools-body">
          {error !== null && (
            <div className="tool-error" role="alert">
              <AlertTriangle size={17} />
              <div>
                <strong>{tr("Operation failed")}</strong>
                <p>{error}</p>
              </div>
            </div>
          )}

          {busy !== null && inspection === null ? (
            <div className="tool-loading">
              <LoaderCircle className="spin" size={21} />
              <span>{tr(busy)}</span>
            </div>
          ) : tab === "health" ? (
            <HealthTab
              inspection={inspection}
              counts={issueCounts}
              busy={busy}
              onRefresh={() => void inspect()}
            />
          ) : tab === "metadata" ? (
            <MetadataTab
              metadata={metadata}
              onChange={setMetadata}
              writeToSource={writeToSource}
              onWriteToSource={setWriteToSource}
              canWriteSource={canWriteSource}
              busy={busy}
              onSave={() => void saveMetadata()}
            />
          ) : (
            <ConvertTab
              format={item.format}
              options={conversion}
              onChange={setConversion}
              busy={busy}
              report={lastReport}
              onConvert={() => void runConversion(false)}
              onRepair={() => void runConversion(true)}
            />
          )}
        </div>
      </aside>
    </>
  );
}

function HealthTab({
  inspection,
  counts,
  busy,
  onRefresh,
}: {
  inspection: PublicationInspection | null;
  counts:
    | { information: number; warning: number; error: number }
    | undefined;
  busy: string | null;
  onRefresh: () => void;
}) {
  if (inspection === null) return null;
  const healthy = (counts?.warning ?? 0) === 0 && (counts?.error ?? 0) === 0;
  return (
    <div className="tool-tab-content">
      <div className={healthy ? "health-summary is-healthy" : "health-summary"}>
        {healthy ? <ShieldCheck size={24} /> : <AlertTriangle size={24} />}
        <div>
          <h3>{tr(healthy ? "No issues found" : "Review recommended")}</h3>
          <p>
            {tr("{{validated}} of {{total}} pages checked", {
              validated: inspection.validatedPages,
              total: inspection.pageCount,
            })} ·{" "}
            {formatBytes(inspection.sourceBytes)}
          </p>
        </div>
      </div>
      <div className="health-facts">
        <span>
          <strong>{inspection.pageCount.toLocaleString(locale())}</strong>
          {tr("Pages")}
        </span>
        <span>
          <strong>{counts?.error ?? 0}</strong>
          {tr("Errors")}
        </span>
        <span>
          <strong>{counts?.warning ?? 0}</strong>
          {tr("Warnings")}
        </span>
        <span>
          <strong>{inspection.duplicateGroups.length}</strong>
          {tr("Duplicate groups")}
        </span>
      </div>
      <div className="tool-section-heading">
        <div>
          <h3>{tr("Findings")}</h3>
        </div>
        <button
          type="button"
          className="secondary-button compact"
          onClick={onRefresh}
          disabled={busy !== null}
        >
          <RefreshCw className={busy !== null ? "spin" : ""} size={14} />
          {tr("Check again")}
        </button>
      </div>
      {inspection.issues.length === 0 ? (
        <div className="tool-empty">
          <CheckCircle2 size={20} />
          {tr("No issues found.")}
        </div>
      ) : (
        <div className="inspection-list">
          {inspection.issues.map((issue, index) => (
            <InspectionRow issue={issue} key={`${issue.code}-${index}`} />
          ))}
        </div>
      )}
    </div>
  );
}

function InspectionRow({ issue }: { issue: InspectionIssue }) {
  const Icon =
    issue.severity === "error"
      ? AlertTriangle
      : issue.severity === "warning"
        ? AlertTriangle
        : Info;
  return (
    <div className={`inspection-row tone-${issue.severity}`}>
      <Icon size={16} />
      <div>
        <strong>{issueTitle(issue)}</strong>
        <p>{issueDescription(issue)}</p>
      </div>
      {issue.pageIndex !== null && (
        <span>{tr("Page {{page}}", { page: issue.pageIndex + 1 })}</span>
      )}
    </div>
  );
}

function MetadataTab({
  metadata,
  onChange,
  writeToSource,
  onWriteToSource,
  canWriteSource,
  busy,
  onSave,
}: {
  metadata: PublicationMetadata | null;
  onChange: (metadata: PublicationMetadata) => void;
  writeToSource: boolean;
  onWriteToSource: (value: boolean) => void;
  canWriteSource: boolean;
  busy: string | null;
  onSave: () => void;
}) {
  if (metadata === null) return null;
  const set = <Key extends keyof PublicationMetadata>(
    key: Key,
    value: PublicationMetadata[Key],
  ) => onChange({ ...metadata, [key]: value });
  return (
    <div className="tool-tab-content metadata-editor">
      <div className="metadata-grid">
        <ToolField label={tr("Title")} wide>
          <input
            value={metadata.title}
            onChange={(event) => set("title", event.target.value)}
          />
        </ToolField>
        <ToolField label={tr("Series")}>
          <input
            value={metadata.series ?? ""}
            onChange={(event) => set("series", nullable(event.target.value))}
          />
        </ToolField>
        <ToolField label={tr("Number")}>
          <input
            value={metadata.number ?? ""}
            onChange={(event) => set("number", nullable(event.target.value))}
          />
        </ToolField>
        <ToolField label={tr("Volume")}>
          <input
            type="number"
            value={metadata.volume ?? ""}
            onChange={(event) =>
              set(
                "volume",
                event.target.value === "" ? null : Number(event.target.value),
              )
            }
          />
        </ToolField>
        <ToolField label={tr("Language")}>
          <input
            value={metadata.language ?? ""}
            onChange={(event) => set("language", nullable(event.target.value))}
            placeholder="en, ja, fr…"
          />
        </ToolField>
        <ToolField label={tr("Writer")}>
          <input
            value={metadata.writer ?? ""}
            onChange={(event) => set("writer", nullable(event.target.value))}
          />
        </ToolField>
        <ToolField label={tr("Artist / penciller")}>
          <input
            value={metadata.penciller ?? ""}
            onChange={(event) => set("penciller", nullable(event.target.value))}
          />
        </ToolField>
        <ToolField label={tr("Publisher")}>
          <input
            value={metadata.publisher ?? ""}
            onChange={(event) => set("publisher", nullable(event.target.value))}
          />
        </ToolField>
        <ToolField label={tr("Reading direction")}>
          <select
            value={metadata.direction}
            onChange={(event) =>
              set(
                "direction",
                event.target.value as PublicationMetadata["direction"],
              )
            }
          >
            <option value="automatic">{tr("Automatic")}</option>
            <option value="rightToLeft">{tr("Right to left")}</option>
            <option value="leftToRight">{tr("Left to right")}</option>
            <option value="vertical">{tr("Vertical")}</option>
          </select>
        </ToolField>
        <ToolField label={tr("Genres")} wide>
          <input
            value={metadata.genres.join(", ")}
            onChange={(event) => set("genres", splitList(event.target.value))}
            placeholder={tr("Drama, Slice of life")}
          />
        </ToolField>
        <ToolField label={tr("Tags")} wide>
          <input
            value={metadata.tags.join(", ")}
            onChange={(event) => set("tags", splitList(event.target.value))}
          />
        </ToolField>
        <ToolField label={tr("Source URL")} wide>
          <input
            type="url"
            value={metadata.web ?? ""}
            onChange={(event) => set("web", nullable(event.target.value))}
          />
        </ToolField>
        <ToolField label={tr("Summary")} wide>
          <textarea
            rows={5}
            value={metadata.summary ?? ""}
            onChange={(event) => set("summary", nullable(event.target.value))}
          />
        </ToolField>
      </div>
      <label className={`source-write-option${canWriteSource ? "" : " is-disabled"}`}>
        <input
          type="checkbox"
          checked={writeToSource && canWriteSource}
          onChange={(event) => onWriteToSource(event.target.checked)}
          disabled={!canWriteSource}
        />
        <span>
          <strong>{tr("Write ComicInfo.xml to source")}</strong>
          <small>
            {tr(
              canWriteSource
                ? "Creates a backup first."
                : "Available for CBZ and image folders.",
            )}
          </small>
        </span>
      </label>
      <div className="tool-footer-actions">
        <button
          type="button"
          className="primary-button"
          onClick={onSave}
          disabled={busy !== null || metadata.title.trim().length === 0}
        >
          {busy !== null ? <LoaderCircle className="spin" size={16} /> : <Save size={16} />}
          {tr("Save metadata")}
        </button>
      </div>
    </div>
  );
}

function ConvertTab({
  format,
  options,
  onChange,
  busy,
  report,
  onConvert,
  onRepair,
}: {
  format: string;
  options: ConversionOptions;
  onChange: (options: ConversionOptions) => void;
  busy: string | null;
  report: ConversionReport | null;
  onConvert: () => void;
  onRepair: () => void;
}) {
  const isPdf = format === "pdf";
  const qualityVisible = options.imageFormat === "jpeg";
  return (
    <div className="tool-tab-content">
      {isPdf && (
        <div className="tool-note">
          <Info size={16} />
          <p>
            {tr("PDF-to-CBZ conversion is unavailable.")}
          </p>
        </div>
      )}
      <fieldset className="conversion-options" disabled={busy !== null || isPdf}>
        <legend>{tr("Page output")}</legend>
        <div className="conversion-format">
          {(
            [
              ["original", tr("Keep original")],
              ["jpeg", "JPEG"],
              ["png", "PNG"],
              ["webp", "WebP"],
            ] as Array<[OutputImageFormat, string]>
          ).map(([value, label]) => (
            <button
              type="button"
              className={options.imageFormat === value ? "is-active" : ""}
              aria-pressed={options.imageFormat === value}
              onClick={() => onChange({ ...options, imageFormat: value })}
              key={value}
            >
              {tr(label)}
            </button>
          ))}
        </div>
        {qualityVisible && (
          <label className="conversion-slider">
            <span>
              {tr("JPEG quality")}
              <output>{options.jpegQuality}</output>
            </span>
            <Slider.Root
              className="slider-root"
              min={50}
              max={100}
              step={1}
              value={[options.jpegQuality]}
              onValueChange={(value) =>
                onChange({ ...options, jpegQuality: value[0] ?? 90 })
              }
            >
              <Slider.Track className="slider-track">
                <Slider.Range className="slider-range" />
              </Slider.Track>
              <Slider.Thumb className="slider-thumb" />
            </Slider.Root>
          </label>
        )}
        <ToolField label={tr("Maximum page dimension")}>
          <select
            value={options.maxDimension ?? ""}
            onChange={(event) =>
              onChange({
                ...options,
                maxDimension:
                  event.target.value === "" ? null : Number(event.target.value),
              })
            }
          >
            <option value="">{tr("Original size")}</option>
            <option value="3200">3200 px</option>
            <option value="2400">2400 px</option>
            <option value="1600">1600 px</option>
            <option value="1200">1200 px</option>
          </select>
        </ToolField>
      </fieldset>
      <div className="conversion-actions">
        <button
          type="button"
          className="primary-button"
          onClick={onConvert}
          disabled={busy !== null || isPdf}
        >
          {busy === "Converting publication…" ? (
            <LoaderCircle className="spin" size={16} />
          ) : (
            <FileCog size={16} />
          )}
          {tr("Convert to CBZ")}
        </button>
        <button
          type="button"
          className="secondary-button"
          onClick={onRepair}
          disabled={busy !== null || isPdf}
        >
          {busy === "Repairing publication…" ? (
            <LoaderCircle className="spin" size={16} />
          ) : (
            <Wrench size={16} />
          )}
          {tr("Repair into new CBZ")}
        </button>
      </div>
      {report !== null && (
        <div className="conversion-report">
          <CheckCircle2 size={19} />
          <div>
            <strong>{tr("CBZ created")}</strong>
            <p>
              {tr("{{count}} pages", { count: report.pageCount })} ·{" "}
              {formatBytes(report.outputBytes)} ·{" "}
              {tr("{{count}} skipped", { count: report.skippedPages.length })}
            </p>
            <code>{report.outputPath}</code>
          </div>
        </div>
      )}
    </div>
  );
}

function ToolField({
  label,
  wide = false,
  children,
}: {
  label: string;
  wide?: boolean;
  children: ReactNode;
}) {
  return (
    <label className={`tool-field${wide ? " is-wide" : ""}`}>
      <span>{label}</span>
      {children}
    </label>
  );
}

function nullable(value: string): string | null {
  const trimmed = value.trim();
  return trimmed.length === 0 ? null : value;
}

function splitList(value: string): string[] {
  return value
    .split(/[,;]/)
    .map((part) => part.trim())
    .filter(Boolean);
}

function safeFileName(value: string): string {
  const cleaned = [...value]
    .map((character) =>
      character.charCodeAt(0) < 32 || '<>:"/\\|?*'.includes(character)
        ? " "
        : character,
    )
    .join("")
    .trim();
  return cleaned.length === 0 ? tr("Untitled") : cleaned.slice(0, 96);
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KiB", "MiB", "GiB", "TiB"];
  let value = bytes / 1024;
  let unit = units[0]!;
  for (let index = 1; value >= 1024 && index < units.length; index += 1) {
    value /= 1024;
    unit = units[index]!;
  }
  return `${value >= 10 ? value.toFixed(0) : value.toFixed(1)} ${unit}`;
}

function issueTitle(issue: InspectionIssue): string {
  const labels: Record<InspectionIssue["code"], string> = {
    metadataIncomplete: "Metadata is sparse",
    duplicateContent: "Duplicate page content",
    extensionMismatch: "Filename and image data differ",
    unreadablePage: "Unreadable page",
    veryLargePage: "Very large page",
    widePages: "Wide pages found",
    pdfManifestOnly: "PDF structure verified",
  };
  return tr(labels[issue.code]);
}

function issueDescription(issue: InspectionIssue): string {
  const descriptions: Record<InspectionIssue["code"], string> = {
    metadataIncomplete: "Only basic title metadata is available.",
    duplicateContent: "Duplicate pages were detected.",
    extensionMismatch: "The filename extension does not match the image data.",
    unreadablePage: "This page could not be read.",
    veryLargePage:
      "This page is larger than 64 MiB and may open slowly on mobile devices.",
    widePages: "Wide pages can use Koma's split or rotate setting.",
    pdfManifestOnly:
      "The PDF structure and page tree are valid. Pages render on demand.",
  };
  return tr(descriptions[issue.code]);
}

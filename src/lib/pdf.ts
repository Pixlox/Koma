import type { PDFDocumentProxy } from "pdfjs-dist/types/src/display/api";

import type { PagePayload } from "../types";

const documents = new Map<string, Promise<PDFDocumentProxy>>();
const MAX_RENDER_PIXELS = 24_000_000;
const importPdfModule = () => import("pdfjs-dist/legacy/build/pdf.mjs");
let pdfModule: ReturnType<typeof importPdfModule> | undefined;

async function loadPdfModule() {
  pdfModule ??= importPdfModule().then((module) => {
    module.GlobalWorkerOptions.workerSrc = new URL(
      "pdfjs-dist/legacy/build/pdf.worker.min.mjs",
      import.meta.url,
    ).toString();
    return module;
  });
  return pdfModule;
}

function documentKey(sourceUrl: string, password: string | null): string {
  return `${sourceUrl}\u0000${password ?? ""}`;
}

async function openPdf(sourceUrl: string, password: string | null) {
  const key = documentKey(sourceUrl, password);
  const existing = documents.get(key);
  if (existing !== undefined) return existing;

  const { getDocument } = await loadPdfModule();
  const loading = getDocument({
    url: sourceUrl,
    password: password ?? undefined,
    isEvalSupported: false,
    useSystemFonts: true,
    stopAtErrors: false,
  }).promise.catch((error: unknown) => {
      documents.delete(key);
      throw error;
    });
  documents.set(key, loading);
  return loading;
}

export async function renderPdfPage(
  sourceUrl: string,
  pageIndex: number,
  password: string | null,
): Promise<PagePayload> {
  const document = await openPdf(sourceUrl, password);
  if (pageIndex < 0 || pageIndex >= document.numPages) {
    throw new Error(`Page ${pageIndex + 1} does not exist`);
  }

  const page = await document.getPage(pageIndex + 1);
  const unscaled = page.getViewport({ scale: 1 });
  const pixelRatio = Math.min(2.5, Math.max(1, window.devicePixelRatio || 1));
  const targetWidth = Math.min(
    2_400,
    Math.max(1_200, window.innerWidth * pixelRatio),
  );
  let scale = targetWidth / Math.max(1, unscaled.width);
  const projectedPixels =
    unscaled.width * scale * unscaled.height * scale;
  if (projectedPixels > MAX_RENDER_PIXELS) {
    scale *= Math.sqrt(MAX_RENDER_PIXELS / projectedPixels);
  }

  const viewport = page.getViewport({ scale });
  const canvas = documentCanvas(viewport.width, viewport.height);
  await page.render({
    canvas,
    viewport,
    background: "rgb(255, 255, 255)",
  }).promise;

  return {
    index: pageIndex,
    mimeType: "image/png",
    dataUrl: canvas.toDataURL("image/png"),
  };
}

export function closePdf(sourceUrl: string): void {
  for (const [key, promise] of documents) {
    if (!key.startsWith(`${sourceUrl}\u0000`)) continue;
    documents.delete(key);
    void promise.then((document) => document.destroy()).catch(() => undefined);
  }
}

function documentCanvas(width: number, height: number): HTMLCanvasElement {
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.floor(width));
  canvas.height = Math.max(1, Math.floor(height));
  return canvas;
}

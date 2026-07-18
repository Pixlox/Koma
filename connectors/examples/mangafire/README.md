# MangaFire native connector

MangaFire is Koma's bundled advanced connector. It is compiled into the app;
this folder documents the implementation as a reference for connector authors.
It is not an installable `.koma-connector.json` package.

The compiled implementation is
[`crates/koma-core/src/importer/mod.rs`](../../../crates/koma-core/src/importer/mod.rs).
Its request flow is recorded in [`request-flow.json`](request-flow.json).

## Why it is native

MangaFire needs behavior beyond the version 1 declarative format:

- a title request and volume list;
- a paginated official-chapter list;
- one detail request for each selected chapter;
- provider-aware pacing and bounded retries for `429` and temporary `5xx`
  responses;
- strict checks that returned title, volume, chapter, language, and page hosts
  match the selected work;
- a verified official-chapter fallback when a volume image fails.

Keeping that logic in Rust lets Koma apply the same URL, DNS, size, image, and
atomic-output checks used by the rest of the importer.

## Import scopes

- **Chapter** loads the official chapter list, downloads the selected chapter,
  and creates one CBZ.
- **Volume** downloads the selected official volume. If a page request fails,
  Koma can reconstruct it only when the official chapter sequence is an exact
  geometric match.
- **Entire series** lists official chapters from earliest to latest, paces
  chapter-detail requests, validates every page, and creates one CBZ after all
  pages have downloaded.

The connector accepts only works the reader owns or has permission to
download. Koma displays that warning and requires confirmation before an
import.

## Patterns worth reusing

For a permitted source with similar requirements:

1. Separate link parsing, metadata resolution, page downloading, and packaging.
2. Bind every response back to the selected work before trusting its URLs.
3. Preserve decimal chapter order.
4. Pace metadata fan-out; retry only temporary responses with a finite budget.
5. Download into a temporary directory and move the completed CBZ into place
   only after validation and packaging succeed.
6. Put host permissions and parser assumptions under tests.

Do not copy MangaFire endpoint assumptions into another connector. Model the
source's documented or authorized interface and keep provider-specific behavior
isolated.


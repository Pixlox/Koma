# Koma connectors

A Koma connector maps a permitted JSON feed into Koma's import model:

`source link → feed → entries → pages → CBZ`

Entries may represent chapters, issues, episodes, image sets, or any other ordered part of a serial publication. This keeps the staged source → entries → pages model useful beyond manga. Connector packages contain data only. They cannot execute code, access Koma's database, read files, start processes, or contact hosts that are not declared in the package.

## Create a connector

1. Copy [`examples/koma-feed-v1.koma-connector.json`](examples/koma-feed-v1.koma-connector.json).
2. Set a unique lowercase `id`, display `name`, and package `version`.
3. Match pasted source links with `sourcePattern`. Named regex captures can be used in `requestUrl` as `$name`.
4. Declare every API and page-image host.
5. Map the JSON response with RFC 6901 JSON pointers.
6. Validate the package against [`connector.schema.json`](connector.schema.json).
7. In Koma, open **Settings → Connectors → Import connector**.

The complete field reference and testing checklist are in
[`AUTHORING.md`](AUTHORING.md). The bundled MangaFire implementation is
documented in [`examples/mangafire/`](examples/mangafire/) as an advanced
multi-request example.

The feed request must return the title and an ordered list of entries. Page lists can be provided in either form:

- Inline: set `mapping.chapterPages` to the page array inside each entry.
- Staged: set `pageRequestUrl` to a URL template and `mapping.pageResponsePages` to the page array in that response. Placeholders such as `{/id}` read values from the current entry with a JSON pointer.

Koma sorts decimal entry numbers numerically, so `0`, `0.5`, `1`, and `1.5` retain the expected order. Both page-list forms support URL strings or objects with URL and optional dimension mappings.

## Network permissions

- HTTPS is required for internet sources.
- `allowedRequestHosts` limits feed requests.
- `allowedPageHosts` limits image downloads.
- A leading `*.` permits subdomains, but not the root domain.
- `allowLocalNetwork` permits HTTP and private addresses for sources running on the user's own network. Koma shows this permission before installation.
- A local feed can therefore run on `localhost` or a private LAN address without giving the connector access to arbitrary files.
- Redirects are not followed.

Koma pins DNS results for each import, limits response and image sizes, validates every downloaded image, caps concurrency, and packages pages only after all checks pass.

## Versioning

`schemaVersion` is the connector API version. Koma currently accepts version `1`. Package `version` belongs to the connector author and can use any short release identifier.

Version 1 targets JSON sources with inline or staged page lists. Future connector schema versions can add new source shapes without changing installed version 1 packages.

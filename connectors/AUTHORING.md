# Connector authoring guide

A connector turns an authorized JSON source into ordered pages that Koma can
package as a CBZ. Version 1 connectors are declarative JSON: they cannot run
scripts or access the local filesystem.

Start with one of these packages:

- [`examples/koma-feed-v1.koma-connector.json`](examples/koma-feed-v1.koma-connector.json)
  for a feed that includes page URLs.
- [`examples/koma-staged-feed-v1.koma-connector.json`](examples/koma-staged-feed-v1.koma-connector.json)
  when each entry has a separate JSON page-list endpoint.

## 1. Match the pasted link

`sourcePattern` is a regular expression for the link a reader pastes into Koma.
Use named captures for values needed by the feed request:

```json
{
  "sourcePattern": "^https://reader\\.example/series/(?P<slug>[a-z0-9-]+)$",
  "requestUrl": "https://api.reader.example/v1/series/$slug"
}
```

Koma accepts the link only when the entire pattern matches. Use narrow patterns
and HTTPS endpoints for internet sources.

## 2. Declare network access

Every request host must be listed:

```json
{
  "allowedRequestHosts": ["api.reader.example"],
  "allowedPageHosts": ["images.reader.example", "*.images.reader.example"],
  "allowLocalNetwork": false
}
```

`*.images.reader.example` permits subdomains but not
`images.reader.example` itself. Add both when both are used.

Set `allowLocalNetwork` only for a connector intentionally designed for
`localhost` or a private LAN service. Koma shows this permission before
installation. Redirects are rejected, so the final host must be declared
directly.

## 3. Map the feed

Mappings use RFC 6901 JSON pointers. Given:

```json
{
  "data": {
    "title": "Example",
    "language": "en",
    "entries": [
      {
        "number": 0.5,
        "volume": 1,
        "pages": [
          {
            "url": "https://images.reader.example/0001.webp",
            "width": 1600,
            "height": 2400
          }
        ]
      }
    ]
  }
}
```

the mapping is:

```json
{
  "mapping": {
    "title": "/data/title",
    "language": "/data/language",
    "chapters": "/data/entries",
    "chapterNumber": "/number",
    "chapterVolume": "/volume",
    "chapterPages": "/pages",
    "pageUrl": "/url",
    "pageWidth": "/width",
    "pageHeight": "/height"
  }
}
```

`language`, `chapterVolume`, `pageUrl`, `pageWidth`, and `pageHeight` are
optional. Without `pageUrl`, each page entry must be a URL string. Without
`language`, Koma records `und` (undetermined).

Entry numbers can be integers or decimals. Koma orders `0`, `0.5`, `1`, and
`1.5` numerically.

## 4. Use a staged page request when needed

When the feed contains entries but not their pages, set `pageRequestUrl` and
`mapping.pageResponsePages`:

```json
{
  "pageRequestUrl": "https://api.reader.example/entries/{/entryId}/pages",
  "mapping": {
    "title": "/work/title",
    "chapters": "/work/entries",
    "chapterNumber": "/number",
    "pageResponsePages": "/data/pages",
    "pageUrl": "/source"
  }
}
```

`{/entryId}` reads `/entryId` from the current entry. Koma may fetch up to six
page lists concurrently, so the source should expose ordinary rate-limit
headers or be sized for that request rate.

## 5. Choose import scopes

`capabilities` controls the choices shown in the importer:

- `chapter` packages one selected entry.
- `volume` groups entries with the same `chapterVolume`.
- `series` packages every entry, earliest to latest.

Every version 1 connector supports `series`. Advertise `volume` only when
`chapterVolume` is mapped.

## 6. Validate and test

Before publishing:

1. Validate the package against [`connector.schema.json`](connector.schema.json).
2. Import it in **Settings → Connectors** and review the displayed host
   permissions.
3. Paste a matching source link and confirm the title, language, entry count,
   page count, and available scopes.
4. Download the smallest available chapter and reopen its CBZ in Koma.
5. Test a non-matching link, a missing JSON field, an invalid image, and a host
   outside the allowlist. Each must stop without leaving a partial CBZ.
6. Test the largest expected series within Koma's page and size limits.

Increase the package `version` when publishing an update. Keep the connector
`id` stable so readers replace the existing installation instead of creating a
duplicate.

## Version 1 limits

Version 1 is intended for stable JSON feeds with one feed request and either
inline pages or one page-list request per entry. Sources requiring pagination,
cookies, browser execution, signed requests, or provider-specific fallback
logic need a native connector. The bundled
[`MangaFire example`](examples/mangafire/) shows that shape.


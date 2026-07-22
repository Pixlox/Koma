# Connector authoring guide

Connectors should use `schemaVersion: 2` and remain a single JSON file. The
filename must end in `.koma-connector.json`.

## Minimal JSON connector

```json
{
  "$schema": "./connector.schema.json",
  "schemaVersion": 2,
  "id": "example-reader",
  "name": "Example Reader",
  "version": "1.0.0",
  "sourcePattern": "^https://reader\\.example/series/(?P<slug>[a-z0-9-]+)$",
  "requestUrl": "https://api.reader.example/series/$slug",
  "allowedRequestHosts": ["api.reader.example"],
  "allowedPageHosts": ["images.reader.example"],
  "allowLocalNetwork": false,
  "responseType": "json",
  "capabilities": ["chapter", "volume", "series"],
  "mapping": {
    "title": "/data/title",
    "language": "/data/language",
    "chapters": "/data/chapters",
    "chapterNumber": "/number",
    "chapterVolume": "/volume",
    "chapterPages": "/pages",
    "pageUrl": "/url",
    "pageWidth": "/width",
    "pageHeight": "/height"
  }
}
```

`id` uses lowercase letters, numbers, and hyphens. `version` is the connector
author's version; `schemaVersion` is Koma's connector API version.

## Match links and build the request

`sourcePattern` is a Rust regular expression. Named captures can be expanded in
`requestUrl`:

```json
{
  "sourcePattern": "^https://reader\\.example/g/(?P<id>[0-9]+)/?$",
  "requestUrl": "https://reader.example/api/galleries/$id"
}
```

Keep the pattern narrow and anchored with `^` and `$`. Koma only considers a
connector when the pasted link matches and the expanded request URL passes its
host policy.

## Declare network access

```json
{
  "allowedRequestHosts": ["api.reader.example"],
  "allowedPageHosts": [
    "images.reader.example",
    "*.images.reader.example"
  ],
  "allowLocalNetwork": false
}
```

Important details:

- Internet requests must use HTTPS.
- `*.example.com` permits subdomains, not `example.com` itself.
- Redirects are not followed.
- Host permissions are checked again after DNS resolution.
- Private, loopback, link-local, and reserved addresses are blocked unless
  `allowLocalNetwork` is true.
- Local-network connectors may use HTTP and should request the smallest
  possible host set.

## Map JSON with pointers

Mappings use RFC 6901 JSON pointers. Given:

```json
{
  "data": {
    "title": "Example",
    "language": "en",
    "chapters": [
      {
        "number": 0.5,
        "volume": 1,
        "pages": [
          {
            "url": "https://images.reader.example/1.webp",
            "width": 1200,
            "height": 1800
          }
        ]
      }
    ]
  }
}
```

use:

```json
{
  "mapping": {
    "title": "/data/title",
    "language": "/data/language",
    "chapters": "/data/chapters",
    "chapterNumber": "/number",
    "chapterVolume": "/volume",
    "chapterPages": "/pages",
    "pageUrl": "/url",
    "pageWidth": "/width",
    "pageHeight": "/height"
  }
}
```

Required mappings are `title`, `chapters`, and `chapterNumber`.

Optional mappings:

- `language`: BCP 47 language code. Defaults to `und`.
- `chapterVolume`: number used to group chapters into volumes.
- `pageUrl`: omit when each page is already a URL string.
- `pageWidth` and `pageHeight`: checked against downloaded image dimensions.

Chapter and volume numbers may be decimals. Koma sorts `0`, `0.5`, `1`, and
`1.5` numerically.

## Separate page-list requests

When the first response contains chapters but not their pages, declare
`pageRequestUrl` and `mapping.pageResponsePages` instead of
`mapping.chapterPages`:

```json
{
  "pageRequestUrl": "https://api.reader.example/chapters/{/id}/pages",
  "mapping": {
    "title": "/title",
    "chapters": "/chapters",
    "chapterNumber": "/number",
    "pageResponsePages": "/pages",
    "pageUrl": "/url"
  }
}
```

Each `{/pointer}` placeholder reads from the current chapter object. Staged page
responses must be JSON. Their request hosts use `allowedRequestHosts`.

## Rhai runtime

Add `transformScript` when the response cannot be mapped directly. Koma runs
the script after the declared `requestUrl` succeeds and before JSON-pointer
mapping.

Available variables:

- `response`: parsed JSON when `responseType` is `json`, otherwise the response
  body as a UTF-8 string.
- `source`: the original pasted link.
- `captures`: a map containing named `sourcePattern` captures.
- `settings`: string values from the optional manifest `settings` object.

The final expression must return a value matching your `mapping`. A convenient
normalized shape is:

```text
{
  title,
  language,
  chapters: [
    {
      number,
      volume,
      pages: [{ url, width, height }]
    }
  ]
}
```

Example:

```json
{
  "schemaVersion": 2,
  "responseType": "json",
  "mapping": {
    "title": "/title",
    "language": "/language",
    "chapters": "/chapters",
    "chapterNumber": "/number",
    "chapterPages": "/pages",
    "pageUrl": "/url"
  },
  "transformScript": "let pages = [];\nfor page in response.pages {\n    pages.push(#{ url: \"https://images.reader.example/\" + page.path });\n}\n#{ title: response.title, language: \"en\", chapters: [#{ number: 1, pages: pages }] }"
}
```

For HTML, use `responseType: "text"` and a Rhai transform. Text responses are
not accepted without a transform.

### Registered helpers

- `sha256(value)` → lowercase hexadecimal digest.
- `hmac_sha256(secret, value)` → lowercase hexadecimal HMAC.
- `base64(value)` → standard Base64.
- `url_encode(value)` → percent-encoded string.
- `regex_capture(value, pattern, group)` → captured string or `""`.
- `regex_find_all(value, pattern)` → array of full matches.
- `html_select(html, selector, attribute)` → selected attribute values.
  Pass `""` as the attribute to collect element text.

- `http(method, url, headers, body)` → response map containing `status`,
  `headers`, `body`, and `json`. `headers` is a Rhai map and `body` is a string.

`http` supports any valid HTTP method, so a connector can paginate, submit a
form or JSON body, carry cookies explicitly through the `Cookie` header, and
branch on status or response content. Koma permits at most 64 script requests
per resolution. Every URL must match `allowedRequestHosts`; HTTPS, DNS, private
network, redirect, timeout, and 32 MiB response limits are enforced by Koma.

Example pagination:

```rhai
let chapters = [];
let page = 1;
loop {
    let result = http(
        "GET",
        `https://api.reader.example/series/${captures.slug}?page=${page}`,
        #{ "Accept": "application/json" },
        ""
    );
    if result.status != 200 { throw `HTTP ${result.status}`; }
    for chapter in result.json.chapters { chapters.push(chapter); }
    if !result.json.has_more { break; }
    page += 1;
}
#{ title: response.title, language: "en", chapters }
```

No other Koma APIs are exposed to Rhai. There is no file, process, environment,
database, dynamic module, `eval`, or raw-socket API. Rhai connectors still run
code: only install files you trust.

## Online reading

Connectors do not need separate online-reader code. The normalized chapter and
page result powers both actions in Koma. **Read online** streams the selected
chapter, volume, or series through the guarded page client. **Download** uses
the same selection and order to build a CBZ. Chapter ranges are preserved for
navigation and AniList/MyAnimeList progress.

## Compatibility

New connectors should use schema v2. Schema v1 remains loadable. Existing v2
files that only use `transformScript` continue to work without changes; the
guarded `http` helper is additive.

### Script limits

Koma will enforce:

- 500,000 Rhai operations.
- Two seconds of wall-clock execution.
- 32 call levels.
- Restricted expression depth.
- Eight MiB strings.
- Page-count-sized arrays.
- 16,384 map entries.
- 32 MiB serialized transform output.

`eval` and `import` are disabled. Scripts receive no filesystem, process,
environment, database, raw-socket, or direct HTTP functions.

## Capabilities

`capabilities` controls the importer choices:

```json
{
  "capabilities": ["chapter", "volume", "series"]
}
```

Every connector must support `series`. Add `chapter` and `volume` only when the
mapped feed provides meaningful choices for them.

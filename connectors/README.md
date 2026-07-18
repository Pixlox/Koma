# Koma connectors

Connectors help Koma import from a web source:

`pasted link → declared request → optional Rhai transform → mapped chapters → validated pages → CBZ`

Each connector is one `.koma-connector.json` file. Readers install it from
**Settings → Connectors → Import connector**. A connector never needs to be
compiled into Koma.

## Connector format

Use `schemaVersion: 2` for new connectors.

Schema v2 has two paths:

- **JSON mapping** for APIs that already expose titles, chapters, and page URLs.
- **Rhai transform** for relative URLs, unusual JSON, computed fields, HTML,
  signatures, and other response normalization.

Schema v1 remains supported for existing declarative JSON connectors. There is
no connector schema v3.

Start with:

- [`examples/koma-feed-v1.koma-connector.json`](examples/koma-feed-v1.koma-connector.json)
  for the simplest JSON feed.
- [`examples/koma-staged-feed-v1.koma-connector.json`](examples/koma-staged-feed-v1.koma-connector.json)
  for a separate page-list request per chapter.
- [`examples/relative-pages-v2.koma-connector.json`](examples/relative-pages-v2.koma-connector.json)
  for a schema v2 Rhai transform.

The complete field and scripting reference is in
[`AUTHORING.md`](AUTHORING.md). The JSON Schema is
[`connector.schema.json`](connector.schema.json).

## Permissions and safety

Every connector declares two host lists:

- `allowedRequestHosts` for metadata and page-list requests.
- `allowedPageHosts` for downloaded page images.

Koma enforces those lists after DNS resolution, rejects undeclared redirects,
requires HTTPS unless local-network access is explicitly enabled, caps response
sizes and concurrency, validates every image, and only writes the final CBZ
after all pages pass validation.

A connector with `transformScript` runs Rhai code inside Koma. Koma shows a
prominent warning before installation. The Rhai environment has operation,
time, recursion, collection, string, and output limits. It is not given
filesystem, process, environment, database, raw-socket, or arbitrary HTTP
functions.

> Be careful with connectors from untrusted sources. Inspect the JSON and Rhai code before installation. Koma does not provide a signature, GPG verification, trust store,
or verified badge.
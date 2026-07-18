# Koma

Koma is a local-first manga and comic reader built from scratch with Rust, Tauri, React, and TypeScript. It keeps the common path small: add a publication, read it, and return to the same page. The library, repair tools, metadata, and reader controls stay available without living on top of the artwork.

## What works

### Reading

- CBZ and ZIP
- CBR and RAR, including password prompts
- CB7 and 7z
- CBT, TAR, TGZ, TBZ2, and TXZ
- Image folders
- Fixed-layout EPUB
- PDF, including encrypted-file prompts and lazy page rendering
- Single page, spreads, continuous, webtoon, panel focus, and presentation modes
- Left-to-right, right-to-left, and metadata-aware automatic direction
- Smart, page, width, height, and original-size fitting
- Wide-page keep, split, and rotate policies
- Margin trim, zoom, page gap, brightness, contrast, saturation, gamma, grayscale, invert, and sharpen
- Keyboard, trackpad, mouse, touch, swipe, scrubber, fullscreen, and wake-lock controls
- Progress, completed and unread states, bookmarks, and bounded page notes

### Library and tools

- Non-invasive local catalogue with SQLite WAL storage
- Windowed grid and list views tested with 20,000 publications, plus search, sort, favorites, hidden items, missing-source detection, and relinking
- Recursive folder scan and operating-system file associations
- `ComicInfo.xml` reading, metadata overrides, and explicit source writes with backup
- Full-page inspection for damaged pages, duplicate content, extension mismatches, very large pages, wide pages, and incomplete metadata
- Atomic conversion or repair to CBZ with original, JPEG, PNG, or WebP pages
- Portable JSON backup and merge restore for library state, progress, bookmarks, import receipts, and metadata
- Command search for navigation and common actions

PDF is currently a reading format, not a PDF-to-CBZ conversion source.

## Link import

MangaFire is bundled as Koma's first connector. Paste a title or volume URL, choose a chapter, volume, or the entire series, choose a destination, and confirm that you have permission to download the work. A series import orders official chapters numerically—including `0` and `0.5`—and packages the earliest through latest chapter into one CBZ.

The importer:

- accepts only the expected MangaFire host and title or volume URL shapes;
- follows no redirects and sends no browser credentials;
- resolves and pins only declared public CDN hosts;
- rejects private, loopback, link-local, reserved, and mixed DNS results;
- applies page, byte, pixel, concurrency, timeout, and retry limits;
- decodes and validates every image before packaging;
- writes through a unique temporary file, atomically publishes the CBZ, reopens it, and hashes the result;
- never completes a partial book or leaves a final destination after denial;
- can recover an unavailable direct page only when one unique sequence of MangaFire's same-language chapter pages matches the complete volume geometry.

This feature does not bypass access controls. Provider changes can make an otherwise valid link unavailable until the bundled connector is updated.

Koma also accepts declarative `.koma-connector.json` packages for permitted JSON feeds. A connector declares the source pattern, request templates, host permissions, capabilities, and JSON mappings for publications, entries, and pages. Packages cannot execute code or read Koma's files or database. Start with the [connector guide and schemas](connectors/README.md).

Languages are bundled from repository locale files and appear automatically in Settings. See the [translation contribution guide](languages/README.md) to add a language through GitHub.

The supplied MangaFire volume example was downloaded, packaged, independently ZIP-tested, reopened by Koma, and sampled after decode. The byte counts and hashes are recorded in [the live MangaFire record](docs/release-evidence/mangafire-live-2026-07-18.json).

## Platform status

| Platform | Current proof |
| --- | --- |
| macOS arm64 | Release `.app` and DMG built; packaged app launched against the Rust backend and a real CBZ |
| iPhone | arm64 simulator app built, installed, launched, and visually inspected; distribution is through TestFlight |
| iPad | The same arm64 simulator app built, installed, launched, and visually inspected; distribution is through TestFlight |
| Windows x64 | Native packaging workflow included; not executed on this Mac because the Windows SDK is unavailable |
| Linux x64 | Native packaging workflow included; not executed on this Mac because no Linux host or container runtime is available |

The macOS artifact is locally ad-hoc linked, not Developer ID signed or notarized. App Store, notarization, and Windows signing require the owner's release credentials. See [release evidence](docs/release-evidence/README.md), including the [complete verification matrix](docs/release-evidence/verification-2026-07-18.json) and [iOS simulator record](docs/release-evidence/ios-simulator-2026-07-18.json), for the exact verified boundary.

Desktop builds check the signed `latest.json` published with GitHub releases. iPhone and iPad builds do not load the desktop updater.

## Development

Requirements:

- Node.js 20 or newer
- Rust 1.93 or newer
- The platform prerequisites required by Tauri 2

```sh
npm ci
npm run dev
```

Run the native desktop shell:

```sh
npm run tauri dev
```

Initialize and build iOS:

```sh
npm run tauri ios init -- --ci
npm run tauri ios build -- --debug --target aarch64-sim --no-sign --ci
```

## Verification

```sh
npm audit
npm run lint
npm run typecheck
npm run i18n:check
npm run test:run
npm run build
npm run test:e2e
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

The browser suite includes automated WCAG A and AA checks for the library, link importer, reader settings, and touch layout. GitHub Actions repeats the quality suite and packages macOS, Windows, and Linux on native runners.

## Desktop releases and updates

The release workflow builds Linux x64, Windows x64, macOS Apple silicon, and macOS Intel. It creates signed updater artifacts and a draft GitHub release when a matching version tag is pushed.

1. Set the version everywhere with `npm run release:version -- 0.2.0`.
2. Run `npm run release:check` and the verification commands above.
3. Commit the release, tag it `v0.2.0`, and push the tag.
4. Review and publish the draft GitHub release.

The repository needs `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` secrets. Keep the private updater key outside the repository. Regular CI packaging uses `src-tauri/tauri.ci.conf.json` and does not require that key. iPhone and iPad releases use TestFlight instead.

The product contract lives in [PRODUCT.md](PRODUCT.md). The implemented visual system lives in [DESIGN.md](DESIGN.md).

## License

MIT

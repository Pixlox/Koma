# Release evidence

This directory contains reproducible verification artifacts for Koma. Run
`npm run test:e2e:update` to refresh the current browser screenshots after the
Playwright interaction suite passes.

| Evidence | What it proves |
| --- | --- |
| [`mangafire-live-2026-07-18.json`](mangafire-live-2026-07-18.json) | The supplied title link resolved 17 official chapters, downloaded all 435 pages, packaged the complete series into one CBZ, passed an independent ZIP test, reopened in Koma, and produced independent BLAKE3 and SHA-256 hashes. |
| [`macos-native-2026-07-18.json`](macos-native-2026-07-18.json) | The final arm64 `.app` and verified DMG hashes, linkage, bundle metadata, real-CBZ reading, and native fullscreen acceptance. |
| [`ios-simulator-2026-07-18.json`](ios-simulator-2026-07-18.json) | A local arm64 debug snapshot, iOS file policy, and native iPhone/iPad simulator launches. iOS is intentionally absent from desktop CI and the desktop updater. |
| [`verification-2026-07-18.json`](verification-2026-07-18.json) | Exact frontend, Rust, accessibility, large-library, live-provider, workflow, and packaging results. |
| [`screenshots/`](screenshots/) | Browser acceptance views plus final empty-library iPhone and iPad simulator captures. No comic pages are committed. |

Windows and Linux package jobs are defined in `.github/workflows/ci.yml`, but
they have not been executed because this local repository has no configured
remote and this Mac has neither a Windows SDK nor a Linux container runtime.
Those platforms still need native runtime and installer acceptance.

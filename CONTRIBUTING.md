# Contributing to KiwiConvert

Thanks for helping. Bug reports, new formats, new tools and fixes are all welcome.

## Set up

You need Windows 10 or 11, Node.js 22+, Rust 1.90+ and the Visual Studio C++ build tools.

```powershell
npm ci
npm run fetch:binaries   # downloads FFmpeg and PDFium into vendor/ (not committed)
npm run tauri dev
```

## Where things are

| Path | What |
| --- | --- |
| `src/` | The interface (React). `src/windows/` has one folder per window: wheel, hub, activity, tools. |
| `src-tauri/src/registry.rs` | Which formats and tools each kind of file gets. Start here to add a format. |
| `src-tauri/src/convert.rs`, `tools.rs` | Turn a wheel choice into work. |
| `src-tauri/src/engines/` | The converters: FFmpeg, images, HEIC, PDF, documents, subtitles, archives. |
| `src-tauri/src/platform/` | Windows integration: the drag gesture, the drop target, window helpers. |
| `installer/` | The installer and uninstaller. |
| `scripts/` | Fetching engines, building the installer, generating third-party notices. |

## Before you open a pull request

```powershell
npm run typecheck
npm test
cd src-tauri
cargo test
cargo test -- --ignored --test-threads 3   # converts real files through every engine
```

- Keep changes focused. One fix or feature per pull request.
- Match the style of the code around you. Run `cargo fmt` on Rust code.
- Converted files must never overwrite the original. Write through `engines::write_atomic`
  and name outputs with `naming::output_for`.
- New dependencies need a reason. If one ships in the app, run `node scripts/notices.mjs` to
  update THIRD_PARTY_NOTICES.md.
- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org), for
  example `fix(pdf): keep rotated page thumbnails inside their cell`.

## Reporting bugs

Open an issue with the file type, what you did, what happened, and the log from
`%LOCALAPPDATA%\io.github.jherobred.kiwiconvert\logs\kiwiconvert.log`. Don't attach files
that contain personal information.

Security problems go through [SECURITY.md](SECURITY.md) instead.

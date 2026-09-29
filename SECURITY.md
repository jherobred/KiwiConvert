# Security policy

## Reporting a vulnerability

Please report security problems privately through
[GitHub's private vulnerability reporting](https://github.com/jherobred/KiwiConvert/security/advisories/new),
not in a public issue. Include the steps or file needed to reproduce it.

## What counts

KiwiConvert opens files people receive from others, so these matter most:

- A crafted file that crashes the app in a way that could run code, or that makes it write
  outside the chosen output folder (for example an archive entry like `..\..\x`).
- Anything that sends data off the PC. The app is meant to make no network requests.
- The installer or uninstaller changing or deleting files it didn't create.

Report bugs in FFmpeg or PDFium themselves to those projects as well.

## Supported versions

Only the latest release gets security fixes.

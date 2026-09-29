// Writes THIRD_PARTY_NOTICES.md: the programs KiwiConvert ships, then every Rust crate and
// npm package compiled into it, with the license files those packages include.
// Usage: node scripts/notices.mjs
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const LICENSE_FILE = /^(licen[cs]e|copying|notice|unrar)[^/\\]*$/i;

function licenseFiles(dir) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir, { withFileTypes: true })
    .filter((e) => e.isFile() && LICENSE_FILE.test(e.name))
    .map((e) => readFileSync(join(dir, e.name), "utf8").replace(/\r\n/g, "\n").trim())
    .filter(Boolean);
}

/** Crates linked into a binary: normal dependencies reachable from the root, on Windows. */
function crates(manifest) {
  const meta = JSON.parse(
    execFileSync(
      "cargo",
      ["metadata", "--format-version", "1", "--locked", "--filter-platform", "x86_64-pc-windows-msvc", "--manifest-path", manifest],
      { maxBuffer: 1 << 28, encoding: "utf8" },
    ),
  );
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
  const packages = new Map(meta.packages.map((p) => [p.id, p]));
  const seen = new Set();
  const walk = (id) => {
    if (seen.has(id)) return;
    seen.add(id);
    for (const dep of nodes.get(id).deps) {
      if (dep.dep_kinds.some((k) => k.kind === null)) walk(dep.pkg);
    }
  };
  walk(meta.resolve.root);
  seen.delete(meta.resolve.root);
  return [...seen].map((id) => {
    const p = packages.get(id);
    return { name: p.name, version: p.version, license: p.license ?? "See license file", texts: licenseFiles(dirname(p.manifest_path)) };
  });
}

/** npm packages bundled into the app's interface (production dependencies). */
function npmPackages() {
  const lock = JSON.parse(readFileSync(join(root, "package-lock.json"), "utf8"));
  return Object.entries(lock.packages)
    .filter(([path, p]) => path.startsWith("node_modules/") && !p.dev)
    .map(([path, p]) => ({
      name: path.slice(path.lastIndexOf("node_modules/") + 13),
      version: p.version,
      license: p.license ?? "See license file",
      texts: licenseFiles(join(root, path)),
    }));
}

const unique = (list) => {
  const byKey = new Map(list.map((p) => [`${p.name}@${p.version}`, p]));
  return [...byKey.values()].sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version));
};

const rust = unique([...crates(join(root, "src-tauri", "Cargo.toml")), ...crates(join(root, "installer", "Cargo.toml"))]);
const npm = unique(npmPackages());

// Identical license texts are printed once, listing every package that uses them.
const texts = new Map();
for (const p of [...rust, ...npm]) {
  for (const text of p.texts) {
    const key = createHash("sha256").update(text).digest("hex");
    if (!texts.has(key)) texts.set(key, { text, users: [] });
    texts.get(key).users.push(`${p.name} ${p.version}`);
  }
}

const table = (list) =>
  ["| Package | Version | License |", "| --- | --- | --- |", ...list.map((p) => `| ${p.name} | ${p.version} | ${p.license} |`)].join("\n");

const out = `# Third-party notices

KiwiConvert is released under the MIT License (see LICENSE). It includes the programs and
libraries below, which keep their own licenses.

## Programs shipped with KiwiConvert

### FFmpeg 9.0.2

- Files: the \`ffmpeg\` folder in the install folder (ffmpeg.exe, ffprobe.exe and their DLLs).
- License: GNU General Public License version 3. This build enables GPL parts such as x264
  and x265. The full license is in \`ffmpeg/LICENSE.txt\`.
- Binaries: the "full_build-shared" build by Gyan Doshi,
  https://github.com/GyanD/codexffmpeg/releases/tag/9.0.2
- Source: https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz. The versions and sources of the
  libraries in this build are listed at https://www.gyan.dev/ffmpeg/builds/.
  If a link stops working, open an issue at https://github.com/jherobred/KiwiConvert/issues
  and we will provide the source.
- KiwiConvert runs FFmpeg as a separate program. It does not link to FFmpeg's libraries.

### PDFium (chromium/8066)

- Files: \`pdfium/pdfium.dll\` in the install folder.
- License: BSD-3-Clause. The libraries built into PDFium (FreeType, ICU, libjpeg-turbo,
  libpng, zlib, OpenJPEG, Little CMS and others) keep their own licenses. All of them are in
  \`pdfium/LICENSE.txt\`.
- Binaries: https://github.com/bblanchon/pdfium-binaries/releases/tag/chromium%2F8066
- Source: https://pdfium.googlesource.com/pdfium/

## Code adapted into KiwiConvert

- The SVG tracer in \`src-tauri/src/engines/trace.rs\` is adapted from VTracer
  (https://github.com/visioncortex/vtracer), licensed MIT or Apache-2.0.

## Rust crates

The source of each crate is available at https://crates.io/crates/NAME/VERSION.

${table(rust)}

## npm packages

These are bundled into the app's interface.

${table(npm)}

## License texts

${[...texts.values()]
  .map(({ text, users }) => `### Used by ${users.join(", ")}\n\n\`\`\`text\n${text.replace(/```/g, "'''")}\n\`\`\``)
  .join("\n\n")}
`;

writeFileSync(join(root, "THIRD_PARTY_NOTICES.md"), out);
console.log(`THIRD_PARTY_NOTICES.md: ${rust.length} crates, ${npm.length} npm packages, ${texts.size} license texts`);

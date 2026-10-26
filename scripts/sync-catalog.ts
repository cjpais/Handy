// The Rust app compiles in a copy of @handy-computer/models' catalog.json
// (src-tauri can't depend on node_modules: cargo-only builds never run bun
// install). This keeps the copy identical to the version pinned in
// package.json.
//
//   bun run sync:catalog    copy the package's catalog.json into src-tauri
//   bun run check:catalog   fail if the copy differs (CI)
import { readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const source = require.resolve("@handy-computer/models/catalog.json");
const target = fileURLToPath(
  new URL("../src-tauri/src/catalog/catalog.json", import.meta.url),
);
// The package's exports don't expose package.json; it sits beside the catalog.
const { version } = JSON.parse(
  readFileSync(join(dirname(source), "package.json"), "utf8"),
) as { version: string };

const expected = readFileSync(source);
// Only a missing copy counts as "differs"; any other read error is rethrown
// rather than overwriting a file that couldn't be read.
let current: Buffer | undefined;
try {
  current = readFileSync(target);
} catch (e) {
  if ((e as NodeJS.ErrnoException).code !== "ENOENT") throw e;
}

if (current?.equals(expected)) {
  console.log(`catalog.json matches @handy-computer/models@${version}`);
} else if (process.argv.includes("--check")) {
  console.error(
    `src-tauri/src/catalog/catalog.json differs from @handy-computer/models@${version}; run \`bun run sync:catalog\``,
  );
  process.exit(1);
} else {
  writeFileSync(target, expected);
  console.log(`catalog.json synced from @handy-computer/models@${version}`);
}

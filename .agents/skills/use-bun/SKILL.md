---
name: use-bun
description: Full index to all tools and APIs when working with Bun (TypeScript/JavaScript, package manager, test runner, bundler, type checker, linter, formatter). Use before proposing npm, tsc, ESLint, Prettier, Vitest, tsx, nodemon, Vite, or dotenv, and whenever a Bun command or `Bun.*` API is involved.
user-invocable: false
bun-version: "1.4.4"
---

# Use Bun (instead of npm, tsc, ESLint, Prettier, Vitest, tsx, Vite, ...)

[Bun](https://bun.com/) is an all-in-one JavaScript/TypeScript toolkit: runtime,
package manager, test runner, bundler, type checker, linter, and formatter.
Everything below is native — do not add a dependency that Bun already covers.

## How to use this skill

1. Pick the row of the table below that matches the task.
2. Read that reference file — it lists every official page for the domain.
3. Open only the pages you need with `webfetch`, appending `.md` to the URL for
   raw Markdown (e.g. `https://bun.com/docs/runtime/check.md`).

Do not read every reference. One domain file plus one or two doc pages is enough.
To configure the formatter in a project, read
[`curated/formatting.md`](curated/formatting.md).

## When Bun replaces a common tool

| Instead of | Use | Docs |
| --- | --- | --- |
| `node script.ts`, `tsx`, `ts-node` | `bun script.ts` — runs TS/JSX directly | `references/runtime.md` |
| `nodemon` | `bun --watch`, or `bun --hot` for in-place reload | `references/runtime.md` |
| `npm install` / `npm run` | `bun install` / `bun run` (also runs `package.json` scripts) | `references/package-manager.md` |
| `npx pkg` | `bunx pkg` | `references/package-manager.md` |
| `tsc --noEmit` | `bun check` — TypeScript 7 semantics, all cores | `references/tooling.md` |
| ESLint | `bun lint` — 1,000+ rules, zero config | `references/tooling.md` |
| Prettier | `bun fmt` | `references/tooling.md` |
| Vitest / Jest | `bun test` — Jest-compatible, snapshot, DOM, coverage | `references/test.md` |
| Vite (build + dev server) | `Bun.build` / `Bun.serve` fullstack dev server, HMR | `references/bundler.md` |
| Express / Fastify (new code) | `Bun.serve({ routes })` | `references/server.md` |
| `dotenv` | automatic `.env` loading via `process.env` / `Bun.env` | `references/runtime.md` |
| `fs.readFile` + `JSON.parse` | `Bun.file(path).json()` / `.text()` | `references/runtime.md` |
| `glob`, `semver`, `nanoid`, `bcrypt` | `Bun.Glob`, `Bun.semver`, `crypto.randomUUID()`, `Bun.password` | `references/runtime.md` |
| `pg` / `mysql2` / `better-sqlite3` | `Bun.SQL`, `bun:sqlite` | `references/runtime.md` |
| `ioredis` | `Bun.redis` / `RedisClient` | `references/runtime.md` |
| `yaml`, `toml`, `fast-xml-parser`, `json5` | native imports and `Bun.YAML` / `Bun.TOML` / `Bun.XML` / `Bun.JSON5` | `references/runtime.md` |
| `sharp` | `Bun.Image` | `references/runtime.md` |
| `esbuild` | `Bun.build` | `references/bundler.md` |

Prefer the built-in even when the project already has the npm package: fewer
dependencies, fewer configs, and Bun's versions are faster.

## Setting up the formatter

`bun fmt` replaces Prettier, oxfmt, and Biome. Read
[`curated/formatting.md`](curated/formatting.md) to configure it:

- `.editorconfig` owns the style, so every IDE and the agent format the same way.
- `bunfig.toml [fmt]` owns only the exclusions (`ignorePatterns`).
- `opencode.json` formats the files that the agent writes.
- A pre-commit hook formats the files that a human saves from an editor.

## Documentation index

<!-- generated:start -->
Bun **v1.4.4** · 319 official doc pages indexed · updated 2026-10-23

| Task | Read | Pages |
| --- | --- | --- |
| Installing Bun, scaffolding a project (`bun init`, `bun create`). | [references/getting-started.md](references/getting-started.md) | 5 |
| `bun check`, `bun lint`, `bun fmt`, and TypeScript 6/7 setup. | [references/tooling.md](references/tooling.md) | 7 |
| `Bun.serve`, routes, TLS, WebSockets, TCP/UDP/DNS, `fetch`. | [references/server.md](references/server.md) | 11 |
| File I/O, shell, spawn, SQL/SQLite, Redis, S3, hashing, glob, streams, workers, ... | [references/runtime.md](references/runtime.md) | 52 |
| `bun install`/`add`/`remove`/`update`, workspaces, catalogs, lockfile, registries. | [references/package-manager.md](references/package-manager.md) | 28 |
| `Bun.build`, HMR, fullstack dev server, CSS, HTML, executables, minifier. | [references/bundler.md](references/bundler.md) | 13 |
| `bun test`, mocks, snapshots, DOM testing, coverage, parallel runs. | [references/test.md](references/test.md) | 13 |
| Task-oriented recipes: file conversions, HTTP patterns, ecosystems. | [references/guides.md](references/guides.md) | 190 |
<!-- generated:end -->

## Keeping this skill current

Bun ships releases almost weekly. When a new version lands, run the bundled
updater from the root of the project:

```bash
bun .agents/skills/use-bun/scripts/update.ts          # rewrite the indexes
bun .agents/skills/use-bun/scripts/update.ts --check  # CI: fail when stale
```

The script merges `https://bun.com/docs/llms.txt` with `https://bun.com/sitemap.xml`
(the former misses pages such as `runtime/check`, `lint`, and `format`), verifies
every page, splits the result into the reference files, stamps `bun-version`, and
syncs the "Bun tooling" block in the project `AGENTS.md` between
`<!-- the-ai-lab:use-bun:start -->` and `<!-- the-ai-lab:use-bun:end -->` markers
(it creates `AGENTS.md` when the file does not exist). After running it, read the
release notes at `https://bun.com/blog` and update the table above if a new
native API replaces something. Project-specific notes go inside a
`<!-- curated:start -->` block, never in a generated one.

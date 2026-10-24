# Bun — Runtime and built-in APIs

> Core runtime APIs: files, processes, databases, binary data, and utilities. Every page is also available as raw Markdown: append `.md` to the URL.

<!-- curated:start -->

<!-- curated:end -->

<!-- generated:start -->
_52 pages · Bun v1.4.4 · updated 2026-10-23. Regenerate with `bun .agents/skills/use-bun/scripts/update.ts`._

- [Bun Runtime](https://bun.com/docs/runtime.md): Execute JavaScript/TypeScript files, package.json scripts, and executable packages with Bun's fast runtime.
- [Archive](https://bun.com/docs/runtime/archive.md): Create and extract tar archives with Bun's fast native implementation
- [Auto-install](https://bun.com/docs/runtime/auto-install.md): Bun's automatic package installation feature for standalone script execution
- [Binary Data](https://bun.com/docs/runtime/binary-data.md): Working with binary data in JavaScript
- [Bun APIs](https://bun.com/docs/runtime/bun-apis.md): Overview of Bun's native APIs available on the Bun global object and built-in modules
- [bunfig.toml](https://bun.com/docs/runtime/bunfig.md): Configure Bun's behavior using its configuration file bunfig.toml
- [C Compiler](https://bun.com/docs/runtime/c-compiler.md): Compile and run C from JavaScript with low overhead
- [Spawn](https://bun.com/docs/runtime/child-process.md): Spawn child processes with `Bun.spawn` or `Bun.spawnSync`
- [Code generation from strings](https://bun.com/docs/runtime/code-generation-from-strings.md): Make eval(), new Function() and every other way a string becomes code throw, for the whole process, with --disallow-code-generation-from-strings.
- [Color](https://bun.com/docs/runtime/color.md): Format colors as CSS, ANSI, numbers, hex strings, and more
- [Console](https://bun.com/docs/runtime/console.md): The console object in Bun
- [Cookies](https://bun.com/docs/runtime/cookies.md): Use Bun's native APIs for working with HTTP cookies
- [Cron](https://bun.com/docs/runtime/cron.md): Schedule and parse cron jobs with Bun
- [CSRF Protection](https://bun.com/docs/runtime/csrf.md): Generate and verify CSRF tokens with Bun's built-in API
- [Debugging](https://bun.com/docs/runtime/debugger.md): Debug your Bun code with an interactive debugger using WebKit Inspector Protocol
- [Environment Variables](https://bun.com/docs/runtime/environment-variables.md): Read and configure environment variables in Bun, including automatic .env file support
- [FFI](https://bun.com/docs/runtime/ffi.md): Use Bun's FFI module to efficiently call native libraries from JavaScript
- [File I/O](https://bun.com/docs/runtime/file-io.md): Bun provides a set of optimized APIs for reading and writing files.
- [File System Router](https://bun.com/docs/runtime/file-system-router.md): Bun provides a fast API for resolving routes against file-system paths
- [File Types](https://bun.com/docs/runtime/file-types.md): File types and loaders supported by Bun's bundler and runtime
- [Glob](https://bun.com/docs/runtime/glob.md): Use Bun's fast native implementation of file globbing
- [Globals](https://bun.com/docs/runtime/globals.md): Use Bun's global objects
- [Hashing](https://bun.com/docs/runtime/hashing.md): Utility functions for hashing and verifying passwords with various cryptographically secure algorithms
- [HTMLRewriter](https://bun.com/docs/runtime/html-rewriter.md): Use Bun's HTMLRewriter to transform HTML documents with CSS selectors
- [Image](https://bun.com/docs/runtime/image.md): Decode, transform, and encode images with a fast native pipeline
- [JSON5](https://bun.com/docs/runtime/json5.md): Use Bun's built-in support for JSON5 files through both runtime APIs and bundler integration
- [JSONL](https://bun.com/docs/runtime/jsonl.md): Parse newline-delimited JSON (JSONL) with Bun's built-in streaming parser
- [JSX](https://bun.com/docs/runtime/jsx.md): Built-in JSX and TSX support in Bun with configurable transpilation options
- [Markdown](https://bun.com/docs/runtime/markdown.md): Parse and render Markdown with Bun's built-in Markdown API, supporting GFM extensions and custom rendering callbacks
- [ModuleGraph](https://bun.com/docs/runtime/module-graph.md): Run many instances of one app in a single process with Bun.ModuleGraph. Each instance gets fresh module state and its own timers and I/O; compiled code is shared.
- [Module Resolution](https://bun.com/docs/runtime/module-resolution.md): How Bun resolves modules and handles imports in JavaScript and TypeScript
- [Node-API](https://bun.com/docs/runtime/node-api.md): Use Bun's Node-API module to build native add-ons to Node.js
- [Node.js Compatibility](https://bun.com/docs/runtime/nodejs-compat.md): Bun's compatibility status with Node.js APIs, modules, and globals
- [Plugins](https://bun.com/docs/runtime/plugins.md): Universal plugin API for extending Bun's runtime and bundler
- [Redis](https://bun.com/docs/runtime/redis.md): Use Bun's native Redis client with a Promise-based API
- [REPL](https://bun.com/docs/runtime/repl.md): An interactive JavaScript and TypeScript REPL with syntax highlighting, history, and tab completion
- [S3](https://bun.com/docs/runtime/s3.md): Bun provides fast, native bindings for interacting with S3-compatible object storage services.
- [Secrets](https://bun.com/docs/runtime/secrets.md): Use Bun's Secrets API to store and retrieve sensitive credentials securely
- [Semver](https://bun.com/docs/runtime/semver.md): Use Bun's semantic versioning API
- [Shell](https://bun.com/docs/runtime/shell.md): Use Bun's shell scripting API to run shell commands from JavaScript
- [SQL](https://bun.com/docs/runtime/sql.md): Bun provides native bindings for working with SQL databases through a unified Promise-based API that supports PostgreSQL, MySQL, and SQLite.
- [SQLite](https://bun.com/docs/runtime/sqlite.md): Bun natively implements a high-performance SQLite3 driver.
- [Streams](https://bun.com/docs/runtime/streams.md): Use Bun's streams API to work with binary data without loading it all into memory at once
- [TOML](https://bun.com/docs/runtime/toml.md): Use Bun's built-in support for TOML files through both runtime APIs and bundler integration
- [Transpiler](https://bun.com/docs/runtime/transpiler.md): Use Bun's transpiler to transpile JavaScript and TypeScript code
- [Utils](https://bun.com/docs/runtime/utils.md): Use Bun's utility functions to work with the runtime
- [Watch Mode](https://bun.com/docs/runtime/watch-mode.md): Automatic reloading in Bun with --watch and --hot modes
- [Web APIs](https://bun.com/docs/runtime/web-apis.md): Web-standard APIs supported by Bun for server-side JavaScript
- [WebView](https://bun.com/docs/runtime/webview.md): Control a headless browser from Bun for automation, testing, and scraping — zero dependencies on macOS, Chrome DevTools Protocol everywhere else
- [Workers](https://bun.com/docs/runtime/workers.md): Use Bun's Workers API to create and communicate with a new JavaScript instance running on a separate thread while sharing I/O resources with the main thread
- [XML](https://bun.com/docs/runtime/xml.md): Use Bun's built-in support for XML through both runtime APIs and bundler integration
- [YAML](https://bun.com/docs/runtime/yaml.md): Use Bun's built-in support for YAML files through both runtime APIs and bundler integration
<!-- generated:end -->

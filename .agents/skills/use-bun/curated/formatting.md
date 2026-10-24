# Bun as the code formatter

Use `bun fmt` as the only formatter of the project. Keep the style rules in
`.editorconfig`, because every IDE reads this file. Agents and humans then get
the same result.

## Which file owns what

| File                   | Owns                                                                |
| :--------------------- | :------------------------------------------------------------------ |
| `.editorconfig`        | The style: indentation, line length, line endings, final newline.   |
| `bunfig.toml` `[fmt]`  | What `.editorconfig` cannot express: `ignorePatterns`, `overrides`. |
| `package.json`         | The `format` script that runs `bun fmt`.                            |
| `opencode.json`        | Formatting of the files that the agent writes.                      |
| `.githooks/pre-commit` | Formatting of the files that a human saves from an editor.          |

Do not put style options in `bunfig.toml`. `bun fmt` reads both files, and
`bunfig.toml` wins on an option that both files set. Two sources of truth give
two different results.

## Write the style once in .editorconfig

```ini
root = true

[*]
indent_style = tab
end_of_line = lf
charset = utf-8
trim_trailing_whitespace = true
insert_final_newline = true
max_line_length = 100
```

`bun fmt` reads `indent_style`, `indent_size`, `tab_width`, `max_line_length`,
and `end_of_line` from this file. `max_line_length` sets the wrap column of the
formatter.

## Exclude the generated trees

Keep the generated and vendored trees out of the formatting scope. Add only the
exclusion to `bunfig.toml`:

```toml
[fmt]
ignorePatterns = [".agents/"]
```

Do not run the formatter on generated output as a post-step. The generator
writes raw text, then the formatter rewrites it. A `bun fmt --check` run then
compares the raw output with the formatted file and always reports a stale
result. Exclude the tree instead.

## Replace the format script in package.json

Remove every formatter dependency and its script. Add one script:

```json
{
	"scripts": {
		"format": "bun fmt"
	}
}
```

Run `bun fmt --check` in CI. The command exits with code 1 when a file needs
formatting.

## Format the files that the agent writes

Add the `formatter` block to `opencode.json`. This block replaces the Prettier,
oxfmt, and Biome built-ins with `bun fmt`:

```jsonc
{
	"$schema": "https://opencode.ai/config.json",
	"formatter": {
		"bun-fmt": {
			"command": ["bun", "fmt", "$FILE"],
			"extensions": [
				".js",
				".jsx",
				".mjs",
				".cjs",
				".ts",
				".tsx",
				".mts",
				".cts",
				".css",
				".json",
				".jsonc",
				".yaml",
				".yml",
				".md",
				".mdx",
				".html",
			],
		},
		"prettier": { "disabled": true },
		"oxfmt": { "disabled": true },
		"biome": { "disabled": true },
	},
}
```

## Format the files that a human saves

OpenCode covers agent saves only. Add a pre-commit hook for editor saves. The
hook formats the staged files and stages them again:

```bash
mkdir -p .githooks
printf '#!/bin/sh\nbun fmt --staged\n' > .githooks/pre-commit
chmod +x .githooks/pre-commit
git config core.hooksPath .githooks
```

Commit the `.githooks/` directory. The hook stops a commit when `bun fmt` exits
with an error, for example on a syntax error in a staged file.

## Known limits

- The formatter does not rewrap Markdown prose. `proseWrap` defaults to
  `"preserve"`, so long lines and hard-wrapped lines stay as the author wrote
  them. Set `proseWrap = "always"` in `bunfig.toml [fmt]` to wrap prose at
  `max_line_length`.
- `bun fmt` does not read `trim_trailing_whitespace` from `.editorconfig`. It
  trims trailing whitespace in Markdown.
- The formatter pads Markdown table cells to align the columns. Expect wide
  diffs on the first run.

## Bun tooling (the-ai-lab)

Bun is the all-in-one JavaScript/TypeScript toolkit for this project. Before any
JS/TS tooling decision — dependencies, scripts, builds, tests, CI — load the
`use-bun` skill at `.agents/skills/use-bun/SKILL.md` and follow it.

- Use `bun` / `bunx` instead of `node` / `npm` / `npx` / `yarn` / `pnpm`.
- Run TypeScript directly with `bun run`; type-check with `bun check`.
- Replace `tsc`, ESLint, Prettier, Jest/Vitest, Vite, tsx, nodemon, and dotenv
  with the Bun built-ins listed in the skill's replacement table.
- Format with `bun fmt`. Keep the style in `.editorconfig` and the exclusions
  in `bunfig.toml`. OpenCode formats on save; `bun fmt --check` gates CI.
- Do not add a package or a config file that covers a built-in Bun feature.

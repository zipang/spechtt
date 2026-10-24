/**
 * Validate the DESIGN.md + design-tokens.css contract against the
 * design-system-tokens rules (skill section 10).
 *
 * Run: bun run design:validate
 */
import { parseContract, validateContract } from "@applied-design-systems/design-tokens";

const root = new URL("..", import.meta.url).pathname;
const designMd = await Bun.file(`${root}DESIGN.md`).text();
const tokensCss = await Bun.file(`${root}design-tokens.css`).text();

const contract = parseContract(designMd, tokensCss);
const issues = validateContract(contract);

const errors = issues.filter((issue) => issue.level === "error");
const warnings = issues.filter((issue) => issue.level === "warning");

for (const issue of issues) {
	const target = issue.target ? ` (${issue.target})` : "";
	console.log(`${issue.level === "error" ? "✗" : "!"} ${issue.code}${target}: ${issue.message}`);
}

if (errors.length > 0) {
	console.error(`\n${errors.length} error(s), ${warnings.length} warning(s).`);
	process.exit(1);
}

console.log(`✓ Design contract valid — ${Object.keys(contract.cssValues).length} tokens, 0 errors.`);

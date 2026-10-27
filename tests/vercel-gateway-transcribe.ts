/**
 * Diagnostic: call the Vercel AI Gateway transcription service exactly the way
 * the Bun STT backend does.
 *
 * Run with:
 *   bun run tests/vercel-gateway-transcribe.ts
 *
 * Bun auto-loads `.env.local`, so no key handling is required beyond exporting
 * `AI_GATEWAY_API_KEY` or `VERCEL_AI_GATEWAY_API_KEY` there.
 *
 * This script bypasses the Rust frontend and the NDJSON worker entirely: if it
 * fails with the same "Invalid API key or token" error, the key itself is
 * rejected by Vercel. If it succeeds, the problem is in the Rust → Bun path.
 */

import { createGateway, transcribe } from "ai";

const MODEL = "mistral/voxtral-mini-transcribe-3";
const AUDIO_PATH = "tests/test-record.wav";

const apiKey =
	process.env.AI_GATEWAY_API_KEY ?? process.env.VERCEL_AI_GATEWAY_API_KEY;

if (apiKey === undefined || apiKey.trim().length === 0) {
	console.error(
		"Missing gateway key. Set AI_GATEWAY_API_KEY or VERCEL_AI_GATEWAY_API_KEY in .env.local.",
	);
	process.exit(2);
}

// Never print the key itself — only enough to identify which variable won.
const source =
	process.env.AI_GATEWAY_API_KEY !== undefined
		? "AI_GATEWAY_API_KEY"
		: "VERCEL_AI_GATEWAY_API_KEY";
console.log(
	`key source: ${source} (len=${apiKey.length}, prefix=${apiKey.slice(0, 4)}, suffix=${apiKey.slice(-4)})`,
);
console.log(`model: ${MODEL}`);
console.log(`audio: ${AUDIO_PATH}`);

const audio = new Uint8Array(await Bun.file(AUDIO_PATH).arrayBuffer());
console.log(`audio bytes: ${audio.byteLength}`);

const gateway = createGateway({ apiKey });
const model = gateway.transcription(MODEL);

try {
	const result = await transcribe({ model, audio });
	console.log("transcription:", JSON.stringify(result.text));
} catch (error) {
	console.error("transcription failed:");
	if (error instanceof Error) {
		console.error(`  name: ${error.name}`);
		console.error(`  message: ${error.message}`);
		// AI SDK errors often carry the provider response on `cause`/`response`.
		const cause = (error as { cause?: unknown }).cause;
		if (cause !== undefined) {
			console.error("  cause:", cause);
		}
		const response = (error as { response?: unknown }).response;
		if (response !== undefined) {
			console.error("  response:", response);
		}
	} else {
		console.error(`  ${String(error)}`);
	}
	process.exit(1);
}

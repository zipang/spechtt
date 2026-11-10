/**
 * Dictation languages offered to the user and forwarded to the providers.
 *
 * The list is data, not a UI decision: `registry.ts` reports it through the capabilities handshake,
 * and both transcription adapters and the cleanup pass (`clean.ts`) read the same entries. Codes
 * are ISO-639-1 so a stored setting is meaningful to every provider.
 */

/** One selectable dictation language. */
export interface LanguageOption {
	/** ISO-639-1 code, e.g. `fr`. */
	readonly code: string;
	/** English name shown in the menu, e.g. `French`. */
	readonly label: string;
}

/**
 * Languages the picker offers, sorted by label.
 *
 * Covers the languages the supported providers publish support for, so the union is what a user can
 * reach through any of them. An empty `code` is not in this list: "Auto" is a UI entry the Rust
 * side represents as an empty setting.
 */
export const LANGUAGE_OPTIONS: readonly LanguageOption[] = [
	{ code: "ar", label: "Arabic" },
	{ code: "zh", label: "Chinese" },
	{ code: "cs", label: "Czech" },
	{ code: "da", label: "Danish" },
	{ code: "nl", label: "Dutch" },
	{ code: "en", label: "English" },
	{ code: "fi", label: "Finnish" },
	{ code: "fr", label: "French" },
	{ code: "de", label: "German" },
	{ code: "el", label: "Greek" },
	{ code: "hi", label: "Hindi" },
	{ code: "hu", label: "Hungarian" },
	{ code: "id", label: "Indonesian" },
	{ code: "it", label: "Italian" },
	{ code: "ja", label: "Japanese" },
	{ code: "ko", label: "Korean" },
	{ code: "no", label: "Norwegian" },
	{ code: "pl", label: "Polish" },
	{ code: "pt", label: "Portuguese" },
	{ code: "ro", label: "Romanian" },
	{ code: "ru", label: "Russian" },
	{ code: "es", label: "Spanish" },
	{ code: "sv", label: "Swedish" },
	{ code: "tr", label: "Turkish" },
	{ code: "uk", label: "Ukrainian" },
	{ code: "vi", label: "Vietnamese" }
];

/**
 * Provider packages that document a `language` provider option.
 *
 * Only these receive it. A package that does not publish the option would have to reject an unknown
 * key, and the AI Gateway does exactly that: its `GatewayProviderOptions` are routing and
 * accounting fields (`order`, `only`, `sort`, `models`, `user`, `tags`, `byok`, `serviceTier`, …)
 * with no `language` among them.
 *
 * https://ai-sdk.dev/providers/ai-sdk-providers/ai-gateway.md § "Gateway Provider Options"
 */
const LANGUAGE_AWARE_PACKAGES: Readonly<Record<string, string>> = {
	openai: "openai",
	mistral: "mistral",
	xai: "xai",
	elevenlabs: "elevenlabs"
};

/**
 * Provider packages that resolve a model through the AI Gateway.
 *
 * A gateway-routed model is served by an upstream provider the gateway picks at request time, so
 * there is no single package whose options apply.
 */
const GATEWAY_PACKAGES = new Set(["gateway", "vercel", "vercel-ai-gateway"]);

/**
 * `providerOptions` carrying the dictation language, or `undefined`.
 *
 * The option is only produced for a provider package that documents it. A gateway-routed model gets
 * `undefined` — deliberately, not by omission: the gateway rejects provider options outside its own
 * set, and forwarding a language through it is what turned every streaming take into
 * `stream_failed`.
 *
 * Forwarding stays best effort for the providers that do receive it: one that does not implement
 * the option still transcribes and reports it through the SDK's `warnings`.
 */
export function languageProviderOptions(
	provider: string,
	language: string | undefined
): Record<string, { language: string }> | undefined {
	const code = language?.trim().toLowerCase();
	if (code === undefined || code.length === 0) {
		return undefined;
	}
	const normalized = provider.trim().toLowerCase();
	if (GATEWAY_PACKAGES.has(normalized)) {
		return undefined;
	}
	const key = LANGUAGE_AWARE_PACKAGES[normalized];
	if (key === undefined) {
		return undefined;
	}
	return { [key]: { language: code } };
}

/**
 * Whether the dictation language can reach the model at all.
 *
 * The worker reports this in the capabilities handshake so the Settings row can say that a
 * gateway-routed catalogue ignores the choice, instead of promising an accuracy gain that never
 * arrives.
 */
export function languageIsForwarded(provider: string): boolean {
	const normalized = provider.trim().toLowerCase();
	return LANGUAGE_AWARE_PACKAGES[normalized] !== undefined;
}

/**
 * English name of an ISO-639-1 code, or `undefined` for a code the picker does not offer.
 *
 * The cleanup prompt names the declared language in prose (`clean.ts`), so it needs the label; the
 * code itself is the fallback there.
 */
export function languageName(code: string): string | undefined {
	const normalized = code.trim().toLowerCase();
	return LANGUAGE_OPTIONS.find((option) => option.code === normalized)?.label;
}

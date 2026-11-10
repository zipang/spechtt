import { describe, expect, test } from "bun:test";
import {
	formatSessionHeader,
	formatPartialRecord,
	PARTIALS_ENV,
	PARTIALS_TAG,
	partialsTraceEnabled
} from "./trace";
import {
	analyzeEvents,
	behaviorOf,
	classifyPair,
	formatReport,
	isSessionHeader,
	parseDump,
	parseDumpLine,
	type TracedEvent
} from "./trace-analyze";

describe("partialsTraceEnabled", () => {
	test("is off unless the variable is exactly 1", () => {
		expect(partialsTraceEnabled({})).toBe(false);
		expect(partialsTraceEnabled({ [PARTIALS_ENV]: "0" })).toBe(false);
		expect(partialsTraceEnabled({ [PARTIALS_ENV]: "true" })).toBe(false);
		expect(partialsTraceEnabled({ [PARTIALS_ENV]: "1" })).toBe(true);
	});
});

describe("formatPartialRecord", () => {
	test("records a partial with its text and length", () => {
		const record = formatPartialRecord(
			{
				type: "partial",
				sessionId: "take-1",
				segment: "seg-1",
				text: "d'une dictée"
			},
			7
		);

		expect(record).not.toBeNull();
		const parsed = JSON.parse(record ?? "{}");
		expect(parsed).toEqual({
			seq: 7,
			type: "partial",
			sessionId: "take-1",
			segment: "seg-1",
			chars: 12,
			text: "d'une dictée"
		});
	});

	test("records a delta under its own field name", () => {
		const parsed = JSON.parse(
			formatPartialRecord({ type: "delta", sessionId: "s", delta: " la" }, 2) ?? "{}"
		);
		expect(parsed.text).toBe(" la");
		expect(parsed.chars).toBe(3);
	});

	test("records a stopped event's full text", () => {
		const parsed = JSON.parse(
			formatPartialRecord({ type: "stopped", sessionId: "s", fullText: "the take" }, 9) ??
				"{}"
		);
		expect(parsed.text).toBe("the take");
	});

	test("stays one physical line even when the text has newlines", () => {
		const record = formatPartialRecord(
			{ type: "partial", sessionId: "s", text: "one\ntwo\tthree" },
			1
		);
		expect(record).not.toBeNull();
		expect(record?.includes("\n")).toBe(false);
		expect(JSON.parse(record ?? "{}").text).toBe("one\ntwo\tthree");
	});

	test("ignores events that carry no transcript", () => {
		expect(formatPartialRecord({ type: "progress", processedSeconds: 1 }, 1)).toBeNull();
		expect(formatPartialRecord({ type: "ready" }, 1)).toBeNull();
		expect(formatPartialRecord({ type: "capabilities" }, 1)).toBeNull();
		expect(formatPartialRecord(null, 1)).toBeNull();
		expect(formatPartialRecord("partial", 1)).toBeNull();
	});
});

describe("formatSessionHeader", () => {
	test("names the model so two takes can be compared", () => {
		const line = formatSessionHeader({
			kind: "live",
			provider: "vercel-ai-gateway",
			model: "microsoft/mai-transcribe-2-streaming",
			language: "fr",
			sampleRate: 16000
		});

		expect(line.startsWith(PARTIALS_TAG)).toBe(true);
		expect(line).toContain("model=microsoft/mai-transcribe-2-streaming");
		expect(line).toContain("language=fr");
		expect(line).toContain("rate=16000");
	});

	test("marks an absent language explicitly", () => {
		const line = formatSessionHeader({
			kind: "file",
			provider: "vercel-ai-gateway",
			model: "m"
		});
		expect(line).toContain("language=-");
		expect(line).not.toContain("rate=");
	});
});

describe("classifyPair", () => {
	test("a cumulative growth is an extend", () => {
		expect(classifyPair("d'une", "d'une dictée").kind).toBe("extend");
		expect(classifyPair("d'une", "d'une dictée").newWords).toBe(1);
	});

	test("the same text again is identical", () => {
		expect(classifyPair("d'une dictée", "d'une dictée").kind).toBe("identical");
		expect(classifyPair("d'une dictée", "  d'une dictée  ").kind).toBe("identical");
	});

	test("a shorter draft is a shrink", () => {
		expect(classifyPair("le workflow va créer", "le workflow").kind).toBe("shrink");
	});

	test("a window sliding over a two-word seam is a slide", () => {
		const classified = classifyPair("ée avec le modèle MAI.", "le modèle MAI. Transcribe");
		expect(classified.kind).toBe("slide");
		expect(classified.overlapWords).toBe(3);
		expect(classified.newWords).toBe(1);
	});

	test("a single shared word is not a slide", () => {
		// Two sentences genuinely can start with the same word.
		expect(classifyPair("Et ensuite le workflow", "Et maintenant autre").kind).toBe("new");
	});

	test("a re-spaced seam still matches, and says so", () => {
		// The provider re-spaces the same words between hypotheses.
		const classified = classifyPair("modèle MAI. Transcribe", "MA I. Trans cribe to");
		expect(classified.kind).toBe("slide");
		expect(classified.retokenized).toBe(true);
		expect(classified.overlapWords).toBeGreaterThanOrEqual(2);
	});

	test("unrelated text opens a new window", () => {
		expect(classifyPair("première phrase", "deuxième phrase").kind).toBe("new");
	});
});

describe("analyzeEvents", () => {
	const partial = (seq: number, segment: string, text: string): TracedEvent => ({
		seq,
		type: "partial",
		sessionId: "take-1",
		segment,
		text,
		chars: text.length
	});

	test("names a cumulative provider", () => {
		const reports = analyzeEvents([
			partial(1, "s", "d'une"),
			partial(2, "s", "d'une dictée"),
			partial(3, "s", "d'une dictée avec")
		]);

		expect(reports).toHaveLength(1);
		expect(reports[0]?.dominant).toBe("extend");
		expect(behaviorOf(reports[0]!)).toBe("cumulative");
		expect(reports[0]?.duplicatedChars).toBe(0);
	});

	test("names a sliding-window provider and measures the duplication", () => {
		const reports = analyzeEvents([
			partial(1, "s", "Début d'une nouvelle dictée avec le modèle MAI."),
			partial(2, "s", "le modèle MAI. Transcribe to"),
			partial(3, "s", "Transcribe to Streaming.")
		]);

		expect(reports[0]?.dominant).toBe("slide");
		expect(behaviorOf(reports[0]!)).toBe("sliding window");
		expect(reports[0]?.maxOverlapWords).toBeGreaterThanOrEqual(2);
		expect(reports[0]?.duplicatedChars).toBeGreaterThan(0);
	});

	test("keeps separate segments apart", () => {
		const reports = analyzeEvents([
			partial(1, "a", "one"),
			partial(2, "a", "one two"),
			partial(3, "b", "three"),
			partial(4, "b", "three four")
		]);

		expect(reports).toHaveLength(2);
		expect(reports.every((report) => report.dominant === "extend")).toBe(true);
	});

	test("ignores events that are not hypotheses", () => {
		const reports = analyzeEvents([
			{ seq: 1, type: "started", sessionId: "take-1" },
			{ seq: 2, type: "committed", sessionId: "take-1", text: "final" },
			{ seq: 3, type: "stopped", sessionId: "take-1", text: "the take" }
		]);
		expect(reports).toHaveLength(0);
	});

	test("reports an empty dump clearly", () => {
		expect(formatReport([])).toContain("SPEECHTT_STT_PARTIALS");
	});
});

describe("parseDump", () => {
	const header = (model: string) =>
		`${PARTIALS_TAG} session kind=live provider=vercel-ai-gateway model=${model} language=fr rate=16000`;
	const partial = (seq: number, text: string) =>
		`${PARTIALS_TAG} ${JSON.stringify({ seq, type: "partial", sessionId: "take-1", segment: "seg:0", text, chars: text.length })}`;

	test("attaches every event to the header above it", () => {
		const events = parseDump(
			[header("a"), partial(1, "one"), header("b"), partial(2, "two")].join("\n")
		);

		expect(events).toHaveLength(2);
		expect(events[0]?.session?.model).toBe("a");
		expect(events[1]?.session?.model).toBe("b");
	});

	// A session id restarts at `take-1` on every launch, so appending two takes
	// to one file is how a user compares models. Without the header in the key
	// the two takes would merge into one stream and the comparison would lie.
	test("keeps two takes apart even though both call their session take-1", () => {
		const dump = [
			header("model-a"),
			partial(1, "Début d'une phrase courte."),
			partial(2, "Début d'une phrase courte. Et la suite."),
			header("model-b"),
			partial(3, "Autre chose totalement différent."),
			partial(4, "Autre chose totalement différent. Puis encore.")
		].join("\n");

		const reports = analyzeEvents(parseDump(dump));

		expect(reports).toHaveLength(2);
		expect(reports.map((report) => report.model)).toEqual(["model-a", "model-b"]);
		expect(reports.every((report) => report.dominant === "extend")).toBe(true);
	});

	test("groups the report by model", () => {
		const dump = [
			header("model-a"),
			partial(1, "un deux trois"),
			partial(2, "un deux trois quatre"),
			header("model-b"),
			partial(3, "un deux trois"),
			partial(4, "un deux trois quatre")
		].join("\n");

		const formatted = formatReport(analyzeEvents(parseDump(dump)));

		expect(formatted).toContain("model-a");
		expect(formatted).toContain("model-b");
		expect(formatted.match(/behaviour=/g)).toHaveLength(2);
	});

	test("a purely cumulative take needs no duplication", () => {
		const dump = [
			header("model-a"),
			partial(1, "un deux trois"),
			partial(2, "un deux trois quatre"),
			partial(3, "un deux trois quatre cinq"),
			`${PARTIALS_TAG} ${JSON.stringify({ seq: 4, type: "stopped", sessionId: "take-1", text: "un deux trois quatre cinq" })}`
		].join("\n");

		const reports = analyzeEvents(parseDump(dump));

		expect(reports[0]?.dominant).toBe("extend");
		expect(reports[0]?.duplicatedFactor).toBe(1);
		expect(reports[0]?.finalChars).toBe("un deux trois quatre cinq".length);
		expect(formatReport(reports)).toContain("shipped reducer=1x");
	});

	// Adding a full stop to words already shown is enough to defeat the shipped
	// `starts_with` test, so even a cumulative provider is not safe from it.
	test("one retroactive full stop already duplicates the window", () => {
		const dump = [
			header("model-a"),
			partial(1, "Et voici"),
			partial(2, "Et voici le début"),
			partial(3, "Et voici. Le début"),
			`${PARTIALS_TAG} ${JSON.stringify({ seq: 4, type: "stopped", sessionId: "take-1", text: "Et voici. Le début" })}`
		].join("\n");

		const reports = analyzeEvents(parseDump(dump));
		const factor = reports[0]?.duplicatedFactor ?? 1;

		expect(reports[0]?.counts.slide + (reports[0]?.counts.new ?? 0)).toBeGreaterThan(0);
		expect(factor).toBeGreaterThan(1);
	});

	// A partial is a hypothesis and a delta is an increment: comparing one against
	// the other describes nothing, so the two channels are reported apart.
	test("reports partials and deltas on separate channels", () => {
		const delta = (seq: number, text: string) =>
			`${PARTIALS_TAG} ${JSON.stringify({ seq, type: "delta", sessionId: "take-1", segment: "seg:0", text })}`;
		const dump = [
			header("model-mixed"),
			partial(1, "Nouveau test"),
			delta(2, "Nouveau"),
			partial(3, " cette fois"),
			delta(4, " test cette"),
			`${PARTIALS_TAG} ${JSON.stringify({ seq: 5, type: "stopped", sessionId: "take-1", text: "Nouveau test cette fois" })}`
		].join("\n");

		const reports = analyzeEvents(parseDump(dump));

		expect(reports.map((report) => report.channel).sort()).toEqual(["delta", "partial"]);
		const formatted = formatReport(reports);
		expect(formatted).toContain("partials=2");
		expect(formatted).toContain("deltas=2");
	});
});

describe("parseDumpLine", () => {
	test("reads a tagged line", () => {
		const parsed = parseDumpLine(
			`${PARTIALS_TAG} {"seq":3,"type":"partial","sessionId":"s","text":"hi"}`
		);
		expect(parsed).not.toBeNull();
		expect(isSessionHeader(parsed!)).toBe(false);
		const event = parsed as TracedEvent;
		expect(event.seq).toBe(3);
		expect(event.text).toBe("hi");
	});

	test("reads a session header", () => {
		const parsed = parseDumpLine(
			`${PARTIALS_TAG} session kind=live provider=p model=m language=fr rate=16000`
		);
		expect(parsed).not.toBeNull();
		expect(isSessionHeader(parsed!)).toBe(true);
		expect(parsed).toMatchObject({
			kind: "live",
			provider: "p",
			model: "m",
			language: "fr",
			sampleRate: 16000
		});
	});

	test("marks an absent language as undefined rather than a dash", () => {
		const parsed = parseDumpLine(
			`${PARTIALS_TAG} session kind=file provider=p model=m language=-`
		);
		expect((parsed as { language?: string }).language).toBeUndefined();
	});

	test("ignores a line with no record", () => {
		expect(parseDumpLine("[stt-worker] error code=x")).toBeNull();
		expect(parseDumpLine("")).toBeNull();
	});

	test("ignores malformed JSON", () => {
		expect(parseDumpLine(`${PARTIALS_TAG} {"seq":`)).toBeNull();
	});
});

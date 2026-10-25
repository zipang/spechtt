/**
 * Bridges the NDJSON audio commands to the AI SDK: a `ReadableStream<string>`
 * of base64 PCM16 chunks with a pause gate and a shared `AbortController`.
 */
export class AudioStreamBridge {
	readonly readable: ReadableStream<string>;

	private readonly abortController = new AbortController();
	private controller: ReadableStreamDefaultController<string> | null = null;
	private isPaused = false;
	private isClosed = false;
	private isAborted = false;

	constructor() {
		this.readable = new ReadableStream<string>({
			start: (controller) => {
				this.controller = controller;
			},
			cancel: () => {
				this.isClosed = true;
			},
		});
	}

	get abortSignal(): AbortSignal {
		return this.abortController.signal;
	}

	get paused(): boolean {
		return this.isPaused;
	}

	get closed(): boolean {
		return this.isClosed;
	}

	get aborted(): boolean {
		return this.isAborted;
	}

	/**
	 * Enqueues one base64 PCM16 chunk. Returns `false` when the chunk was
	 * dropped because the stream is paused, closed, or aborted.
	 */
	pushAudio(pcm16Base64: string): boolean {
		if (this.isPaused || this.isClosed || this.isAborted) {
			return false;
		}
		if (this.controller === null) {
			return false;
		}

		try {
			this.controller.enqueue(pcm16Base64);
			return true;
		} catch {
			// The consumer cancelled or the stream errored underneath us.
			this.isClosed = true;
			return false;
		}
	}

	/** Drops audio frames until `resume()` (e.g. while the user holds pause). */
	pause(): void {
		this.isPaused = true;
	}

	resume(): void {
		this.isPaused = false;
	}

	/** Ends the input stream so the provider flushes its final transcript. */
	close(): void {
		if (this.isClosed) {
			return;
		}
		this.isClosed = true;

		try {
			this.controller?.close();
		} catch {
			// Already closed by the consumer.
		}
	}

	/** Tears the session down abruptly (e.g. `stdin` EOF). */
	abort(reason?: unknown): void {
		if (this.isClosed) {
			return;
		}
		this.isClosed = true;
		this.isAborted = true;

		this.abortController.abort(reason);
		try {
			this.controller?.error(
				reason ?? new DOMException("Aborted", "AbortError"),
			);
		} catch {
			// The stream was already errored or cancelled.
		}
	}
}

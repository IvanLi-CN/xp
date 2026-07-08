import { throwIfNotOk } from "./backendError";

export type SseMessage = {
	event: string;
	data: string;
};

export type StartSseStreamArgs = {
	url: string;
	headers?: HeadersInit;
	onMessage: (msg: SseMessage) => void;
	onOpen?: () => void;
	onError?: (err: unknown) => void;
	onClose?: () => void;
	retryDelayMs?: number;
	maxRetryDelayMs?: number;
};

export type SseStreamHandle = {
	close: () => void;
};

function parseSseFrame(frame: string): SseMessage | null {
	let event = "";
	const dataLines: string[] = [];

	for (const rawLine of frame.split("\n")) {
		const line = rawLine.replace(/\r$/, "");
		if (!line || line.startsWith(":")) continue;

		if (line.startsWith("event:")) {
			event = line.slice("event:".length).trim();
			continue;
		}
		if (line.startsWith("data:")) {
			dataLines.push(line.slice("data:".length).trimStart());
		}
	}

	const data = dataLines.join("\n");
	if (!event || !data) return null;
	return { event, data };
}

export function startSseStream(args: StartSseStreamArgs): SseStreamHandle {
	const controller = new AbortController();
	const retryBaseMs = args.retryDelayMs ?? 1_000;
	const maxRetryDelayMs = args.maxRetryDelayMs ?? 10_000;

	function sleep(ms: number) {
		return new Promise<void>((resolve) => {
			if (ms <= 0) {
				resolve();
				return;
			}
			const timeoutId = window.setTimeout(() => {
				controller.signal.removeEventListener("abort", onAbort);
				resolve();
			}, ms);
			function onAbort() {
				window.clearTimeout(timeoutId);
				resolve();
			}
			controller.signal.addEventListener("abort", onAbort, { once: true });
		});
	}

	void (async () => {
		let attempt = 0;
		while (!controller.signal.aborted) {
			try {
				const res = await fetch(args.url, {
					method: "GET",
					headers: {
						Accept: "text/event-stream",
						...args.headers,
					},
					signal: controller.signal,
				});

				await throwIfNotOk(res);
				args.onOpen?.();
				attempt = 0;

				const body = res.body;
				if (!body) throw new Error("SSE response has no body.");

				const reader = body.getReader();
				const decoder = new TextDecoder();
				let buffer = "";

				while (true) {
					const { done, value } = await reader.read();
					if (done) break;

					buffer += decoder.decode(value, { stream: true });
					buffer = buffer.replaceAll("\r\n", "\n");

					let splitIndex = buffer.indexOf("\n\n");
					while (splitIndex !== -1) {
						const frame = buffer.slice(0, splitIndex);
						buffer = buffer.slice(splitIndex + 2);
						const msg = parseSseFrame(frame);
						if (msg) args.onMessage(msg);
						splitIndex = buffer.indexOf("\n\n");
					}
				}

				if (controller.signal.aborted) return;
				args.onClose?.();
			} catch (err) {
				if (controller.signal.aborted) return;
				args.onError?.(err);
				args.onClose?.();
			}

			if (controller.signal.aborted) return;
			const retryMs = Math.min(
				maxRetryDelayMs,
				retryBaseMs * 2 ** Math.min(attempt, 4),
			);
			attempt += 1;
			await sleep(retryMs);
		}
	})();

	return {
		close: () => controller.abort(),
	};
}

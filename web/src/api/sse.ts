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

	void (async () => {
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

			args.onClose?.();
		} catch (err) {
			if (!controller.signal.aborted) {
				args.onError?.(err);
				args.onClose?.();
			}
		}
	})();

	return {
		close: () => controller.abort(),
	};
}

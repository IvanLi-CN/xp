import { afterEach, describe, expect, it, vi } from "vitest";

import { startSseStream } from "./sse";

function streamFromText(text: string, options?: { close?: boolean }) {
	const close = options?.close ?? true;
	return new ReadableStream<Uint8Array>({
		start(controller) {
			controller.enqueue(new TextEncoder().encode(text));
			if (close) controller.close();
		},
	});
}

async function waitFor(assertion: () => void, timeoutMs = 1_000) {
	const startedAt = Date.now();
	for (;;) {
		try {
			assertion();
			return;
		} catch (error) {
			if (Date.now() - startedAt >= timeoutMs) throw error;
			await new Promise((resolve) => window.setTimeout(resolve, 5));
		}
	}
}

afterEach(() => {
	vi.unstubAllGlobals();
});

describe("startSseStream", () => {
	it("reconnects after the stream closes unexpectedly", async () => {
		const fetchMock = vi
			.fn()
			.mockResolvedValueOnce(
				new Response(streamFromText('event: hello\ndata: {"seq":1}\n\n'), {
					status: 200,
					headers: { "Content-Type": "text/event-stream" },
				}),
			)
			.mockResolvedValueOnce(
				new Response(
					streamFromText('event: hello\ndata: {"seq":2}\n\n', {
						close: false,
					}),
					{
						status: 200,
						headers: { "Content-Type": "text/event-stream" },
					},
				),
			);
		vi.stubGlobal("fetch", fetchMock);

		const onMessage = vi.fn();
		const onOpen = vi.fn();
		const onClose = vi.fn();
		const handle = startSseStream({
			url: "/events/demo",
			onMessage,
			onOpen,
			onClose,
			retryDelayMs: 1,
			maxRetryDelayMs: 1,
		});

		await waitFor(() => {
			expect(fetchMock).toHaveBeenCalledTimes(2);
			expect(onOpen).toHaveBeenCalledTimes(2);
			expect(onMessage).toHaveBeenCalledTimes(2);
			expect(onClose).toHaveBeenCalledTimes(1);
		});

		handle.close();
	});
});

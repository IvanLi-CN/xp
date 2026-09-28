import { throwIfNotOk } from "./backendError";

export const READ_REQUEST_TIMEOUT_MS = 8_000;

export class ApiRequestTimeoutError extends Error {
	readonly timeoutMs: number;

	constructor(timeoutMs: number) {
		super(`Request timed out after ${timeoutMs / 1000} seconds.`);
		this.name = "ApiRequestTimeoutError";
		this.timeoutMs = timeoutMs;
	}
}

export class ApiResponseError extends Error {
	constructor() {
		super("The server returned an empty or invalid JSON response.");
		this.name = "ApiResponseError";
	}
}

export async function fetchJsonWithTimeout<T>(
	input: RequestInfo | URL,
	init: RequestInit,
	parse: (value: unknown) => T,
	timeoutMs = READ_REQUEST_TIMEOUT_MS,
): Promise<T> {
	const controller = new AbortController();
	let timedOut = false;
	const timeoutId = setTimeout(() => {
		timedOut = true;
		controller.abort();
	}, timeoutMs);
	const callerSignal = init.signal;
	const abortCaller = () => controller.abort(callerSignal?.reason);

	callerSignal?.addEventListener("abort", abortCaller, { once: true });
	if (callerSignal?.aborted) controller.abort(callerSignal.reason);

	try {
		const response = await fetch(input, {
			...init,
			signal: controller.signal,
		});
		await throwIfNotOk(response);

		let json: unknown;
		try {
			json = await response.json();
		} catch (error) {
			if (timedOut) throw error;
			if (callerSignal?.aborted) throw error;
			if (!(error instanceof SyntaxError)) throw error;
			throw new ApiResponseError();
		}
		try {
			return parse(json);
		} catch {
			throw new ApiResponseError();
		}
	} catch (error) {
		if (timedOut) throw new ApiRequestTimeoutError(timeoutMs);
		throw error;
	} finally {
		clearTimeout(timeoutId);
		callerSignal?.removeEventListener("abort", abortCaller);
	}
}

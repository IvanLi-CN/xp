import { z } from "zod";

export const BackendErrorResponseSchema = z.object({
	error: z.object({
		code: z.string(),
		message: z.string(),
		details: z.record(z.string(), z.unknown()),
	}),
});

export type BackendErrorResponse = z.infer<typeof BackendErrorResponseSchema>;

export class BackendApiError extends Error {
	readonly status: number;
	readonly code?: string;
	readonly details?: Record<string, unknown>;
	readonly retryAfterSeconds?: number;
	readonly retryAfterDeadline?: number;

	constructor(args: {
		status: number;
		message: string;
		code?: string;
		details?: Record<string, unknown>;
		retryAfterSeconds?: number;
		retryAfterDeadline?: number;
	}) {
		super(args.message);
		this.name = "BackendApiError";
		this.status = args.status;
		this.code = args.code;
		this.details = args.details;
		this.retryAfterSeconds = args.retryAfterSeconds;
		const detailRetryAfter = args.details?.retry_after_seconds;
		const retryAfterSeconds =
			args.retryAfterSeconds ??
			(typeof detailRetryAfter === "number" &&
			Number.isInteger(detailRetryAfter) &&
			detailRetryAfter > 0
				? Math.min(detailRetryAfter, 300)
				: undefined);
		this.retryAfterDeadline =
			args.retryAfterDeadline ??
			(retryAfterSeconds ? Date.now() + retryAfterSeconds * 1000 : undefined);
	}
}

export function isBackendApiError(err: unknown): err is BackendApiError {
	return err instanceof BackendApiError;
}

export async function throwIfNotOk(res: Response): Promise<void> {
	if (res.ok) return;

	const text = await res.text();
	let json: unknown = undefined;
	try {
		json = JSON.parse(text);
	} catch {
		json = undefined;
	}

	const parsed = BackendErrorResponseSchema.safeParse(json);
	if (parsed.success) {
		throw new BackendApiError({
			status: res.status,
			code: parsed.data.error.code,
			message: parsed.data.error.message,
			details: parsed.data.error.details,
			retryAfterSeconds: parseRetryAfter(res.headers.get("Retry-After")),
		});
	}

	throw new BackendApiError({
		status: res.status,
		message: `request failed: ${res.status}`,
		retryAfterSeconds: parseRetryAfter(res.headers.get("Retry-After")),
	});
}

function parseRetryAfter(value: string | null): number | undefined {
	if (!value) return undefined;
	const seconds = Number(value);
	return Number.isInteger(seconds) && seconds > 0
		? Math.min(seconds, 300)
		: undefined;
}

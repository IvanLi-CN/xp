import { z } from "zod";

import { throwIfNotOk } from "./backendError";

export const AdminMihomoRedactSourceKindSchema = z.enum(["text", "url"]);
export type AdminMihomoRedactSourceKind = z.infer<
	typeof AdminMihomoRedactSourceKindSchema
>;

export const AdminMihomoRedactionLevelSchema = z.enum([
	"minimal",
	"credentials",
	"credentials_and_address",
]);
export type AdminMihomoRedactionLevel = z.infer<
	typeof AdminMihomoRedactionLevelSchema
>;

export const AdminMihomoSourceFormatSchema = z.enum([
	"auto",
	"raw",
	"base64",
	"yaml",
]);
export type AdminMihomoSourceFormat = z.infer<
	typeof AdminMihomoSourceFormatSchema
>;

export const AdminMihomoRedactRequestSchema = z.object({
	source_kind: AdminMihomoRedactSourceKindSchema,
	source: z.string(),
	level: AdminMihomoRedactionLevelSchema,
	source_format: AdminMihomoSourceFormatSchema,
});

export type AdminMihomoRedactRequest = z.infer<
	typeof AdminMihomoRedactRequestSchema
>;

export const AdminMihomoRedactResponseSchema = z.object({
	redacted_text: z.string(),
});

export type AdminMihomoRedactResponse = z.infer<
	typeof AdminMihomoRedactResponseSchema
>;

export async function redactAdminMihomo(
	adminToken: string,
	payload: AdminMihomoRedactRequest,
	signal?: AbortSignal,
): Promise<AdminMihomoRedactResponse> {
	const res = await fetch("/api/admin/tools/mihomo/redact", {
		method: "POST",
		headers: {
			Accept: "application/json",
			Authorization: `Bearer ${adminToken}`,
			"Content-Type": "application/json",
		},
		body: JSON.stringify(payload),
		signal,
	});

	await throwIfNotOk(res);

	const json: unknown = await res.json();
	return AdminMihomoRedactResponseSchema.parse(json);
}

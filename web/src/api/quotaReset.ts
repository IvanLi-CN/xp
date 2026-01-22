import { z } from "zod";

export const QuotaResetSourceSchema = z.enum(["user", "node"]);
export type QuotaResetSource = z.infer<typeof QuotaResetSourceSchema>;

export const UserQuotaResetSchema = z.union([
	z.object({
		policy: z.literal("unlimited"),
		tz_offset_minutes: z.number().int(),
	}),
	z.object({
		policy: z.literal("monthly"),
		day_of_month: z.number().int().min(1).max(31),
		tz_offset_minutes: z.number().int(),
	}),
]);

export type UserQuotaReset = z.infer<typeof UserQuotaResetSchema>;

export const NodeQuotaResetSchema = z.union([
	z.object({
		policy: z.literal("unlimited"),
		tz_offset_minutes: z.number().int().nullable().optional(),
	}),
	z.object({
		policy: z.literal("monthly"),
		day_of_month: z.number().int().min(1).max(31),
		tz_offset_minutes: z.number().int().nullable().optional(),
	}),
]);

export type NodeQuotaReset = z.infer<typeof NodeQuotaResetSchema>;

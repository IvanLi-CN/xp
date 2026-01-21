import { z } from "zod";

export const VlessCredentialsSchema = z.object({
	uuid: z.string(),
	email: z.string(),
});

export type VlessCredentials = z.infer<typeof VlessCredentialsSchema>;

export const Ss2022CredentialsSchema = z.object({
	method: z.string(),
	password: z.string(),
});

export type Ss2022Credentials = z.infer<typeof Ss2022CredentialsSchema>;

export const GrantCredentialsSchema = z.object({
	vless: VlessCredentialsSchema.optional(),
	ss2022: Ss2022CredentialsSchema.optional(),
});

export type GrantCredentials = z.infer<typeof GrantCredentialsSchema>;

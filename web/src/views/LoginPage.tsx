import { zodResolver } from "@hookform/resolvers/zod";
import { useNavigate } from "@tanstack/react-router";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";

import { verifyAdminToken } from "../api/adminAuth";
import { isBackendApiError } from "../api/backendError";
import { Button } from "../components/Button";
import {
	ADMIN_TOKEN_STORAGE_KEY,
	clearAdminToken,
	readAdminToken,
	writeAdminToken,
} from "../components/auth";
import {
	Form,
	FormControl,
	FormField,
	FormItem,
	FormLabel,
	FormMessage,
} from "../components/ui/form";
import { Input } from "../components/ui/input";
import { parseAdminTokenInput } from "../utils/adminToken";

function formatError(err: unknown): string {
	if (isBackendApiError(err)) {
		const code = err.code ? ` ${err.code}` : "";
		return `${err.status}${code}: ${err.message}`;
	}
	if (err instanceof Error) return err.message;
	return String(err);
}

const loginSchema = z.object({
	token: z
		.string()
		.min(1, "Token is required.")
		.superRefine((value, ctx) => {
			const parsed = parseAdminTokenInput(value);
			if ("error" in parsed) {
				ctx.addIssue({
					code: z.ZodIssueCode.custom,
					message: parsed.error,
				});
			}
		}),
});

type LoginValues = z.infer<typeof loginSchema>;

export function LoginPage() {
	const navigate = useNavigate();
	const storedToken = useMemo(() => readAdminToken(), []);
	const [tokenLength, setTokenLength] = useState(storedToken.length);
	const [isVerifying, setIsVerifying] = useState(false);
	const [serverError, setServerError] = useState<string | null>(null);
	const form = useForm<LoginValues>({
		resolver: zodResolver(loginSchema),
		defaultValues: {
			token: storedToken,
		},
	});

	const submitToken = useCallback(
		async (rawToken: string) => {
			const parsed = parseAdminTokenInput(rawToken);
			if ("error" in parsed) {
				form.setError("token", { message: parsed.error });
				return;
			}

			setIsVerifying(true);
			setServerError(null);
			try {
				await verifyAdminToken(parsed.token);
				writeAdminToken(parsed.token);
				setTokenLength(parsed.token.length);
				form.reset({ token: parsed.token });
				navigate({ to: "/" });
			} catch (err) {
				setServerError(formatError(err));
			} finally {
				setIsVerifying(false);
			}
		},
		[form, navigate],
	);

	useEffect(() => {
		const params = new URLSearchParams(window.location.search);
		const loginToken = params.get("login_token");
		if (!loginToken) return;

		params.delete("login_token");
		const nextQuery = params.toString();
		const nextUrl = `${window.location.pathname}${
			nextQuery.length ? `?${nextQuery}` : ""
		}${window.location.hash ?? ""}`;
		window.history.replaceState(null, "", nextUrl);

		form.reset({ token: loginToken });
		void submitToken(loginToken);
	}, [form, submitToken]);

	return (
		<div className="flex min-h-screen items-center justify-center bg-muted/35 px-6 py-10">
			<div className="xp-card w-full max-w-lg">
				<div className="xp-card-body space-y-5">
					<div className="flex items-start gap-3">
						<img
							src="/xp-mark.png"
							alt=""
							aria-hidden="true"
							className="size-12 shrink-0"
						/>
						<div className="space-y-1">
							<h1 className="text-2xl font-semibold tracking-tight">
								Admin login
							</h1>
							<p className="text-sm text-muted-foreground">
								Enter the admin token to access the admin UI.
							</p>
						</div>
					</div>

					<div className="space-y-2">
						<p className="text-xs uppercase tracking-[0.18em] text-muted-foreground">
							Stored in localStorage key
						</p>
						<div className="rounded-2xl border border-border/70 bg-muted/55 px-4 py-3">
							<p className="font-mono text-sm">{ADMIN_TOKEN_STORAGE_KEY}</p>
						</div>
					</div>

					<Form {...form}>
						<form
							className="space-y-4"
							onSubmit={form.handleSubmit((values) =>
								submitToken(values.token),
							)}
						>
							<FormField
								control={form.control}
								name="token"
								render={({ field }) => (
									<FormItem>
										<FormLabel>Token</FormLabel>
										<FormControl>
											<Input
												{...field}
												type="password"
												placeholder="e.g. admin-token"
												className="font-mono"
												onChange={(event) => {
													setServerError(null);
													field.onChange(event);
												}}
											/>
										</FormControl>
										<FormMessage />
									</FormItem>
								)}
							/>

							{tokenLength === 0 ? (
								<div className="rounded-xl border border-warning/30 bg-warning/10 px-4 py-3 text-sm text-foreground">
									<p className="font-medium text-warning-foreground">
										No token set.
									</p>
									<p className="mt-1 text-muted-foreground">
										Ask an administrator for a token or a temporary login link.
									</p>
								</div>
							) : (
								<p className="text-sm text-muted-foreground">
									Token stored (length {tokenLength}).
								</p>
							)}

							{serverError ? (
								<div className="rounded-xl border border-destructive/30 bg-destructive/10 px-4 py-3 text-sm text-destructive">
									{serverError}
								</div>
							) : null}

							<div className="flex flex-wrap justify-end gap-2">
								<Button
									variant="ghost"
									onClick={() => {
										clearAdminToken();
										setTokenLength(0);
										setServerError(null);
										form.reset({ token: "" });
									}}
								>
									Clear
								</Button>
								<Button
									type="submit"
									variant="secondary"
									loading={isVerifying}
									disabled={isVerifying}
								>
									Save &amp; Continue
								</Button>
							</div>
						</form>
					</Form>
				</div>
			</div>
		</div>
	);
}

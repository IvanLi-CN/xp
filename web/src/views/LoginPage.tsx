import { zodResolver } from "@hookform/resolvers/zod";
import { useNavigate } from "@tanstack/react-router";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";

import { verifyAdminToken } from "../api/adminAuth";
import { isBackendApiError } from "../api/backendError";
import { usePrimaryBackend } from "../backend/PrimaryBackendProvider";
import {
	getPrimaryBackendSnapshot,
	hydratePrimaryBackendProfile,
	isStaticWebConsole,
	probeBackendOrigin,
	selectPreAuthBackend,
} from "../backend/primaryBackend";
import { Button } from "../components/Button";
import { LoginBackendSelector } from "../components/LoginBackendSelector";
import { XpBrandLogo } from "../components/XpBrandLogo";
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
import { sanitizeRedirectPath } from "../utils/navigation";
import { LoginBootstrapCompatibilityPending } from "./LoginBootstrapCompatibilityPending";

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

type LoginPageProps = {
	staticConsole?: boolean;
};

export function LoginPage({
	staticConsole = isStaticWebConsole(),
}: LoginPageProps) {
	const navigate = useNavigate();
	const backend = usePrimaryBackend();
	const [preAuthTargetReady, setPreAuthTargetReady] = useState(staticConsole);
	const storedToken = useMemo(() => readAdminToken(), []);
	const [tokenLength, setTokenLength] = useState(storedToken.length);
	const [isVerifying, setIsVerifying] = useState(false);
	const [serverError, setServerError] = useState<string | null>(null);
	const [bootstrapCompatibilityPending, setBootstrapCompatibilityPending] =
		useState(false);
	const redirectTarget = useMemo(() => {
		const params = new URLSearchParams(window.location.search);
		return sanitizeRedirectPath(params.get("redirect"));
	}, []);
	const form = useForm<LoginValues>({
		resolver: zodResolver(loginSchema),
		defaultValues: {
			token: storedToken,
		},
	});
	const draftToken = form.watch("token");
	const hasUnverifiedToken = tokenLength === 0 && draftToken.length > 0;

	const submitToken = useCallback(
		async (rawToken: string) => {
			const parsed = parseAdminTokenInput(rawToken);
			if ("error" in parsed) {
				form.setError("token", { message: parsed.error });
				return;
			}

			setIsVerifying(true);
			setServerError(null);
			setBootstrapCompatibilityPending(false);
			const verificationTarget = getPrimaryBackendSnapshot();
			try {
				await verifyAdminToken(parsed.token);
				const currentTarget = getPrimaryBackendSnapshot();
				if (
					currentTarget.primaryOrigin !== verificationTarget.primaryOrigin ||
					currentTarget.clusterId !== verificationTarget.clusterId ||
					currentTarget.generation !== verificationTarget.generation
				) {
					setServerError(
						"Service node changed while the token was being verified. Try again.",
					);
					return;
				}
				writeAdminToken(parsed.token);
				setTokenLength(parsed.token.length);
				form.reset({ token: parsed.token });
				navigate({ href: redirectTarget });
			} catch (err) {
				if (staticConsole && err instanceof TypeError) {
					setBootstrapCompatibilityPending(true);
				} else {
					setServerError(formatError(err));
				}
			} finally {
				setIsVerifying(false);
			}
		},
		[form, navigate, redirectTarget, staticConsole],
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
		let active = true;
		setIsVerifying(true);
		void (async () => {
			try {
				const initialTarget = getPrimaryBackendSnapshot();
				if (!staticConsole) {
					const probeResult = await probeBackendOrigin({
						origin: window.location.origin,
					});
					if (!active) return;
					const currentTarget = getPrimaryBackendSnapshot();
					if (currentTarget.primaryOrigin !== initialTarget.primaryOrigin) {
						setServerError(
							"Service node selection changed. Review the selected node and retry.",
						);
						return;
					}
					if (probeResult.clusterId !== currentTarget.clusterId) {
						hydratePrimaryBackendProfile(probeResult.clusterId);
					}
					const selectionResult = await selectPreAuthBackend(
						window.location.origin,
					);
					if (selectionResult.timedOut) {
						setServerError(
							"The previous write result is unknown. Verify the service node before retrying.",
						);
						return;
					}
				}
				if (!active) return;
				await submitToken(loginToken);
			} catch (error) {
				setServerError(
					error instanceof Error
						? error.message
						: "Unable to select the current service node.",
				);
			} finally {
				if (active) setIsVerifying(false);
			}
		})();
		return () => {
			active = false;
		};
	}, [form, staticConsole, submitToken]);

	const handleFormSubmit = form.handleSubmit((values) => {
		if (!staticConsole && backend.state === "switching") {
			setServerError("Service node switching is still in progress.");
			return;
		}
		if (!staticConsole && !preAuthTargetReady) {
			setServerError("Verify the selected service node before continuing.");
			return;
		}
		void submitToken(values.token);
	});

	return (
		<div className="flex min-h-screen items-center justify-center bg-muted/35 px-6 py-10">
			<div className="xp-card w-full max-w-lg">
				<div className="xp-card-body space-y-5">
					<div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:gap-4">
						<XpBrandLogo
							kind="lockup"
							className="h-10 w-[8.5rem] shrink-0 sm:h-12 sm:w-[10rem]"
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

					<LoginBackendSelector
						disabled={isVerifying}
						staticConsole={staticConsole}
						onProbeStateChange={setPreAuthTargetReady}
					/>

					<div className="space-y-2">
						<p className="text-xs uppercase tracking-[0.18em] text-muted-foreground">
							Stored in localStorage key
						</p>
						<div className="rounded-2xl border border-border/70 bg-muted/55 px-4 py-3">
							<p className="font-mono text-sm">{ADMIN_TOKEN_STORAGE_KEY}</p>
						</div>
					</div>

					<Form {...form}>
						<form className="space-y-4" onSubmit={handleFormSubmit}>
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
													setBootstrapCompatibilityPending(false);
													field.onChange(event);
												}}
											/>
										</FormControl>
										<FormMessage />
									</FormItem>
								)}
							/>

							{tokenLength > 0 ? (
								<p className="text-sm text-muted-foreground">
									Token stored (length {tokenLength}).
								</p>
							) : hasUnverifiedToken ? (
								<p className="text-sm text-muted-foreground">
									Token is not saved until verification succeeds.
								</p>
							) : (
								<div className="xp-alert xp-alert-warning px-4 py-3">
									<p className="font-medium">No token set.</p>
									<p className="mt-1 text-muted-foreground">
										Ask an administrator for a token or a temporary login link.
									</p>
								</div>
							)}

							{bootstrapCompatibilityPending ? (
								<LoginBootstrapCompatibilityPending />
							) : serverError ? (
								<div role="alert" className="xp-alert xp-alert-error px-4 py-3">
									{serverError}
								</div>
							) : null}

							<div className="flex flex-wrap justify-end gap-2">
								<Button
									variant="ghost"
									disabled={isVerifying}
									onClick={() => {
										clearAdminToken();
										setTokenLength(0);
										setServerError(null);
										setBootstrapCompatibilityPending(false);
										form.reset({ token: "" });
									}}
								>
									Clear
								</Button>
								<Button
									type="submit"
									variant="secondary"
									loading={isVerifying}
									disabled={
										isVerifying ||
										(!staticConsole &&
											(!preAuthTargetReady || backend.state === "switching"))
									}
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
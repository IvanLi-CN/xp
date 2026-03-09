import { zodResolver } from "@hookform/resolvers/zod";
import { Link, useNavigate } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";

import { createAdminUser } from "../api/adminUsers";
import { isBackendApiError } from "../api/backendError";
import type { UserQuotaReset } from "../api/quotaReset";
import { Button } from "../components/Button";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { useToast } from "../components/Toast";
import { readAdminToken } from "../components/auth";
import {
	Form,
	FormControl,
	FormField,
	FormItem,
	FormLabel,
	FormMessage,
} from "../components/ui/form";
import { Input } from "../components/ui/input";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "../components/ui/select";

function formatError(err: unknown): string {
	if (isBackendApiError(err)) {
		const code = err.code ? ` ${err.code}` : "";
		return `${err.status}${code}: ${err.message}`;
	}
	if (err instanceof Error) return err.message;
	return String(err);
}

const createUserSchema = z
	.object({
		displayName: z.string().trim().min(1, "Display name is required."),
		resetPolicy: z.enum(["monthly", "unlimited"]),
		resetDay: z.coerce.number().int().min(1).max(31),
		resetTzOffsetMinutes: z.coerce
			.number({ invalid_type_error: "tz_offset_minutes must be a number." })
			.int("tz_offset_minutes must be an integer."),
	})
	.superRefine((values, ctx) => {
		if (
			values.resetPolicy === "monthly" &&
			(values.resetDay < 1 || values.resetDay > 31)
		) {
			ctx.addIssue({
				code: z.ZodIssueCode.custom,
				path: ["resetDay"],
				message: "Reset day must be between 1 and 31.",
			});
		}
	});

type CreateUserValues = z.infer<typeof createUserSchema>;

export function UserNewPage() {
	const adminToken = readAdminToken();
	const navigate = useNavigate();
	const { pushToast } = useToast();
	const [serverError, setServerError] = useState<string | null>(null);
	const form = useForm<CreateUserValues>({
		resolver: zodResolver(createUserSchema),
		defaultValues: {
			displayName: "",
			resetPolicy: "monthly",
			resetDay: 1,
			resetTzOffsetMinutes: 480,
		},
	});
	const resetPolicy = form.watch("resetPolicy");
	const isSubmitting = form.formState.isSubmitting;

	const content = useMemo(() => {
		if (adminToken.length === 0) {
			return (
				<PageState
					variant="empty"
					title="Admin token required"
					description="Set an admin token to create users."
					action={
						<Button asChild>
							<Link to="/login">Go to login</Link>
						</Button>
					}
				/>
			);
		}

		return (
			<div className="xp-card">
				<div className="xp-card-body space-y-4">
					<Form {...form}>
						<form
							className="space-y-4"
							onSubmit={form.handleSubmit(async (values) => {
								setServerError(null);
								try {
									const quotaReset: UserQuotaReset =
										values.resetPolicy === "monthly"
											? {
													policy: "monthly",
													day_of_month: values.resetDay,
													tz_offset_minutes: values.resetTzOffsetMinutes,
												}
											: {
													policy: "unlimited",
													tz_offset_minutes: values.resetTzOffsetMinutes,
												};

									const created = await createAdminUser(adminToken, {
										display_name: values.displayName.trim(),
										quota_reset: quotaReset,
									});
									pushToast({ variant: "success", message: "User created." });
									navigate({
										to: "/users/$userId",
										params: { userId: created.user_id },
									});
								} catch (err) {
									setServerError(formatError(err));
									pushToast({
										variant: "error",
										message: "Failed to create user.",
									});
								}
							})}
						>
							<FormField
								control={form.control}
								name="displayName"
								render={({ field }) => (
									<FormItem>
										<FormLabel>Display name</FormLabel>
										<FormControl>
											<Input {...field} placeholder="e.g. Customer A" />
										</FormControl>
										<FormMessage />
									</FormItem>
								)}
							/>

							<div className="grid gap-4 md:grid-cols-2">
								<FormField
									control={form.control}
									name="resetPolicy"
									render={({ field }) => (
										<FormItem>
											<FormLabel>Quota reset policy</FormLabel>
											<Select
												onValueChange={(value) => {
													setServerError(null);
													field.onChange(value);
												}}
												defaultValue={field.value}
											>
												<FormControl>
													<SelectTrigger>
														<SelectValue />
													</SelectTrigger>
												</FormControl>
												<SelectContent>
													<SelectItem value="monthly">monthly</SelectItem>
													<SelectItem value="unlimited">unlimited</SelectItem>
												</SelectContent>
											</Select>
											<FormMessage />
										</FormItem>
									)}
								/>

								<FormField
									control={form.control}
									name="resetDay"
									render={({ field }) => (
										<FormItem>
											<FormLabel>Reset day of month</FormLabel>
											<FormControl>
												<Input
													{...field}
													type="number"
													min={1}
													max={31}
													disabled={resetPolicy !== "monthly"}
													onChange={(event) =>
														field.onChange(event.target.value)
													}
												/>
											</FormControl>
											<FormMessage />
										</FormItem>
									)}
								/>

								<FormField
									control={form.control}
									name="resetTzOffsetMinutes"
									render={({ field }) => (
										<FormItem className="md:col-span-2">
											<FormLabel>User tz_offset_minutes</FormLabel>
											<FormControl>
												<Input
													{...field}
													type="number"
													onChange={(event) =>
														field.onChange(event.target.value)
													}
												/>
											</FormControl>
											<FormMessage />
										</FormItem>
									)}
								/>
							</div>

							{serverError ? (
								<div className="rounded-xl border border-destructive/30 bg-destructive/10 px-4 py-3 text-sm text-destructive">
									{serverError}
								</div>
							) : null}

							<div className="flex justify-end">
								<Button type="submit" loading={isSubmitting}>
									Create user
								</Button>
							</div>
						</form>
					</Form>
				</div>
			</div>
		);
	}, [
		adminToken,
		form,
		isSubmitting,
		navigate,
		pushToast,
		resetPolicy,
		serverError,
	]);

	return (
		<div className="space-y-6">
			<PageHeader
				title="New user"
				description="Create a new subscription owner."
				actions={
					<Button asChild variant="ghost" size="sm">
						<Link to="/users">Back</Link>
					</Button>
				}
			/>
			{content}
		</div>
	);
}

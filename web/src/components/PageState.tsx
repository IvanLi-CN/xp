import type { ReactNode } from "react";

import { Card, CardContent, CardTitle } from "@/components/ui/card";

import { AuthRecoveryAction, isUnauthorizedError } from "./AuthRecoveryAction";

type PageStateVariant = "loading" | "empty" | "error" | "offline";

type PageStateProps = {
	variant: PageStateVariant;
	title: string;
	description?: string;
	error?: unknown;
	action?: ReactNode;
};

const variantTone: Record<PageStateVariant, string> = {
	loading: "text-info",
	empty: "text-foreground",
	error: "text-destructive",
	offline: "text-warning",
};

export function PageState({
	variant,
	title,
	description,
	error,
	action,
}: PageStateProps) {
	const needsSignIn = variant === "error" && isUnauthorizedError(error);
	const hasAction = needsSignIn || action;

	return (
		<Card>
			<CardContent className="flex items-center justify-center px-6 py-12 text-center">
				<div className="flex max-w-lg flex-col items-center gap-4">
					<CardTitle className={variantTone[variant]}>{title}</CardTitle>
					{description ? (
						<p className="text-sm text-muted-foreground">{description}</p>
					) : null}
					{variant === "loading" ? (
						<div className="xp-loading-spinner size-8 border-[3px]" />
					) : null}
					{hasAction ? (
						<div className="flex flex-wrap items-center justify-center gap-2">
							{needsSignIn ? <AuthRecoveryAction error={error} /> : null}
							{action}
						</div>
					) : null}
				</div>
			</CardContent>
		</Card>
	);
}

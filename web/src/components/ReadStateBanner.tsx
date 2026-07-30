import { useState } from "react";

import { AuthRecoveryAction, isUnauthorizedError } from "./AuthRecoveryAction";
import { Button } from "./Button";
import { Icon } from "./Icon";
import { alertClass } from "./ui-helpers";

type ReadStateBannerProps = {
	tone: "info" | "warning";
	title: string;
	description?: string;
	variant?: "banner" | "inline";
	dismissible?: boolean;
	error?: unknown;
};

export function ReadStateBanner({
	tone,
	title,
	description,
	variant = "banner",
	dismissible = false,
	error,
}: ReadStateBannerProps) {
	const [dismissed, setDismissed] = useState(false);

	if (dismissed) {
		return null;
	}

	const iconName =
		tone === "warning" ? "tabler:alert-circle" : "tabler:info-circle";

	const dismissButton = dismissible ? (
		<Button
			variant="ghost"
			size="sm"
			className={
				"size-7 shrink-0 rounded-full px-0 text-foreground/60 " +
				"hover:bg-background/40 hover:text-foreground"
			}
			aria-label={`Dismiss ${title}`}
			onClick={() => setDismissed(true)}
		>
			<Icon name="tabler:x" size={16} className="shrink-0" />
		</Button>
	) : null;
	const recoveryAction =
		tone === "info" && isUnauthorizedError(error) ? (
			<AuthRecoveryAction error={error} />
		) : null;
	const actions =
		recoveryAction || dismissButton ? (
			<div className="flex shrink-0 items-center gap-2">
				{recoveryAction}
				{dismissButton}
			</div>
		) : null;

	if (variant === "inline") {
		return (
			<div
				className={alertClass(
					tone,
					"items-center justify-between gap-2 rounded-lg px-3 py-2",
				)}
			>
				<div className="flex min-w-0 items-start gap-2">
					<Icon
						name={iconName}
						size={16}
						className="mt-0.5 shrink-0 opacity-80"
					/>
					<div className="min-w-0">
						<div className="flex flex-wrap items-baseline gap-x-2 gap-y-0.5">
							<p className="font-medium leading-5">{title}</p>
							{description ? (
								<p className="text-xs leading-5 text-foreground/75">
									{description}
								</p>
							) : null}
						</div>
					</div>
				</div>
				{actions}
			</div>
		);
	}

	return (
		<div className={alertClass(tone, "justify-between")}>
			<div className="flex min-w-0 items-start gap-3">
				<Icon
					name={iconName}
					size={18}
					className="mt-0.5 shrink-0 opacity-80"
				/>
				<div className="space-y-1">
					<p className="font-semibold">{title}</p>
					{description ? (
						<p className="text-sm text-foreground/80">{description}</p>
					) : null}
				</div>
			</div>
			{actions}
		</div>
	);
}

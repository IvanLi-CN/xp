import type { ComponentPropsWithoutRef, ReactNode } from "react";

import { useUiPrefsOptional } from "./UiPrefs";

type ButtonVariant = "primary" | "secondary" | "ghost";
type ButtonSize = "md" | "sm";

export interface ButtonProps extends ComponentPropsWithoutRef<"button"> {
	variant?: ButtonVariant;
	size?: ButtonSize;
	loading?: boolean;
	iconLeft?: ReactNode;
}

export function Button({
	variant = "primary",
	size,
	loading = false,
	iconLeft,
	disabled,
	children,
	className,
	...rest
}: ButtonProps) {
	const prefs = useUiPrefsOptional();
	const effectiveSize: ButtonSize =
		size ?? (prefs?.density === "compact" ? "sm" : "md");

	const variantClass =
		variant === "secondary"
			? "btn-secondary"
			: variant === "ghost"
				? "btn-ghost"
				: "btn-primary";

	return (
		<button
			type="button"
			className={[
				"btn",
				variantClass,
				effectiveSize === "sm" ? "btn-sm" : "",
				loading ? "btn-disabled" : "",
				className,
			]
				.filter(Boolean)
				.join(" ")}
			disabled={disabled || loading}
			{...rest}
		>
			{loading ? <span className="loading loading-spinner" /> : iconLeft}
			{children}
		</button>
	);
}

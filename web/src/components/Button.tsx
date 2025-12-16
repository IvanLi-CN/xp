import type { ComponentPropsWithoutRef, ReactNode } from "react";

type ButtonVariant = "primary" | "secondary" | "ghost";

export interface ButtonProps extends ComponentPropsWithoutRef<"button"> {
	variant?: ButtonVariant;
	loading?: boolean;
	iconLeft?: ReactNode;
}

export function Button({
	variant = "primary",
	loading = false,
	iconLeft,
	disabled,
	children,
	className,
	...rest
}: ButtonProps) {
	const variantClass =
		variant === "secondary"
			? "btn-secondary"
			: variant === "ghost"
				? "btn-ghost"
				: "btn-primary";

	return (
		<button
			type="button"
			className={["btn", variantClass, loading ? "btn-disabled" : "", className]
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

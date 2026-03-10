import type { ReactNode } from "react";

import {
	Button as UiButton,
	type ButtonProps as UiButtonProps,
} from "@/components/ui/button";
import { cn } from "@/lib/utils";

import { useUiPrefsOptional } from "./UiPrefs";

type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";
type ButtonSize = "md" | "sm";

export interface ButtonProps extends Omit<UiButtonProps, "variant" | "size"> {
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
	asChild,
	type,
	disabled,
	children,
	className,
	...rest
}: ButtonProps) {
	const prefs = useUiPrefsOptional();
	const effectiveSize: ButtonSize =
		size ?? (prefs?.density === "compact" ? "sm" : "md");

	const variantMap = {
		primary: "default",
		secondary: "outline",
		ghost: "ghost",
		danger: "destructive",
	} as const;

	if (asChild) {
		return (
			<UiButton
				asChild
				variant={variantMap[variant]}
				size={effectiveSize === "sm" ? "sm" : "default"}
				className={cn(className)}
				disabled={disabled || loading}
				{...rest}
			>
				{children}
			</UiButton>
		);
	}

	return (
		<UiButton
			type={type ?? "button"}
			variant={variantMap[variant]}
			size={effectiveSize === "sm" ? "sm" : "default"}
			className={cn(className)}
			disabled={disabled || loading}
			{...rest}
		>
			{loading ? (
				<span className="xp-loading-spinner xp-loading-spinner-sm" />
			) : (
				iconLeft
			)}
			{children}
		</UiButton>
	);
}

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

export type IconButtonProps = Omit<ButtonProps, "children" | "iconLeft"> & {
	label: string;
	tooltip?: string;
	children: ReactNode;
};

/** A stable 32px action target for dense row-level controls. */
export function IconButton({
	label,
	tooltip,
	children,
	className,
	...props
}: IconButtonProps) {
	return (
		<Button
			{...props}
			size="sm"
			className={cn("size-8 min-h-8 min-w-8 shrink-0 p-0", className)}
			aria-label={label}
			title={tooltip ?? label}
		>
			{children}
		</Button>
	);
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

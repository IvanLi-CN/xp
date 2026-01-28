import { useState } from "react";

import { Button, type ButtonProps } from "./Button";
import { Icon } from "./Icon";

type CopyButtonProps = {
	text: string;
	label?: string;
	copiedLabel?: string;
	errorLabel?: string;
	ariaLabel?: string;
	iconOnly?: boolean;
	variant?: "primary" | "secondary" | "ghost";
	size?: ButtonProps["size"];
	className?: string;
};

function sleep(ms: number) {
	return new Promise((resolve) => {
		setTimeout(resolve, ms);
	});
}

export function CopyButton({
	text,
	label = "Copy",
	copiedLabel = "Copied",
	errorLabel = "Copy failed",
	ariaLabel,
	iconOnly = false,
	variant = "secondary",
	size,
	className,
}: CopyButtonProps) {
	const [state, setState] = useState<"idle" | "copied" | "error">("idle");

	const displayLabel =
		state === "copied" ? copiedLabel : state === "error" ? errorLabel : label;
	const iconName =
		state === "copied"
			? "tabler:check"
			: state === "error"
				? "tabler:alert-circle"
				: "tabler:copy";
	const iconClassName =
		state === "copied"
			? "text-success"
			: state === "error"
				? "text-error"
				: "opacity-70";

	return (
		<Button
			variant={variant}
			size={size}
			className={className}
			iconLeft={
				<Icon
					name={iconName}
					size={16}
					className={iconClassName}
					ariaLabel={displayLabel}
				/>
			}
			aria-label={ariaLabel ?? displayLabel}
			onClick={async () => {
				try {
					await navigator.clipboard.writeText(text);
					setState("copied");
					await sleep(1200);
					setState("idle");
				} catch {
					setState("error");
					await sleep(1600);
					setState("idle");
				}
			}}
		>
			{iconOnly ? null : displayLabel}
		</Button>
	);
}

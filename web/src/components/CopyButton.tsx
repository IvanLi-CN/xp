import { useState } from "react";

import { Button, type ButtonProps } from "./Button";

type CopyButtonProps = {
	text: string;
	label?: string;
	copiedLabel?: string;
	errorLabel?: string;
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
	variant = "secondary",
	size,
	className,
}: CopyButtonProps) {
	const [state, setState] = useState<"idle" | "copied" | "error">("idle");

	const displayLabel =
		state === "copied" ? copiedLabel : state === "error" ? errorLabel : label;

	return (
		<Button
			variant={variant}
			size={size}
			className={className}
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
			{displayLabel}
		</Button>
	);
}

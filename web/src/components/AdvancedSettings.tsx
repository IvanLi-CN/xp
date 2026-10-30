import type { ReactNode } from "react";
import { cn } from "../lib/utils";
import { Icon } from "./Icon";

type AdvancedSettingsProps = {
	title: string;
	children: ReactNode;
	defaultOpen?: boolean;
};

export function AdvancedSettings({
	title,
	children,
	defaultOpen = false,
}: AdvancedSettingsProps) {
	return (
		<details
			open={defaultOpen}
			className="group rounded-lg border border-border/70 bg-muted/20"
		>
			<summary
				className={cn(
					"flex cursor-pointer list-none items-center justify-between gap-3 rounded-lg px-4 py-3",
					"text-sm font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
					"[&::-webkit-details-marker]:hidden",
				)}
			>
				<span>{title}</span>
				<Icon
					name="tabler:chevron-down"
					size={18}
					ariaLabel=""
					className="shrink-0 transition-transform group-open:rotate-180"
				/>
			</summary>
			<div className="border-t border-border/70 px-4 pb-4 pt-1">{children}</div>
		</details>
	);
}

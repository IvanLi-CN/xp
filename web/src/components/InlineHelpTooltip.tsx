import type { ReactNode } from "react";

import { Icon } from "./Icon";
import {
	Tooltip,
	TooltipContent,
	TooltipProvider,
	TooltipTrigger,
} from "./ui/tooltip";

type InlineHelpTooltipProps = {
	label: string;
	children: ReactNode;
	className?: string;
	contentClassName?: string;
	side?: "top" | "right" | "bottom" | "left";
};

const DEFAULT_HELP_BUTTON_CLASS = [
	"inline-flex size-7 shrink-0 items-center justify-center rounded-md",
	"text-muted-foreground hover:bg-accent hover:text-accent-foreground",
	"focus-visible:outline-none focus-visible:ring-[3px] focus-visible:ring-ring/20",
].join(" ");

export function InlineHelpTooltip({
	label,
	children,
	className,
	contentClassName,
	side,
}: InlineHelpTooltipProps) {
	return (
		<TooltipProvider delayDuration={0}>
			<Tooltip>
				<TooltipTrigger asChild>
					<button
						aria-label={label}
						className={className ?? DEFAULT_HELP_BUTTON_CLASS}
						type="button"
					>
						<Icon ariaLabel="" name="tabler:info-circle" size={16} />
					</button>
				</TooltipTrigger>
				<TooltipContent className={contentClassName ?? "max-w-sm"} side={side}>
					{children}
				</TooltipContent>
			</Tooltip>
		</TooltipProvider>
	);
}

import { type VariantProps, cva } from "class-variance-authority";
import type * as React from "react";

import { cn } from "@/lib/utils";

const badgeVariants = cva(
	"inline-flex items-center gap-1 rounded-full border px-2.5 py-0.5 text-xs font-medium transition-colors",
	{
		variants: {
			variant: {
				default: "border-transparent bg-primary/12 text-primary",
				secondary: "border-transparent bg-secondary text-secondary-foreground",
				success:
					"border-success/40 bg-success/22 text-success-foreground ring-1 ring-success/25 dark:bg-success/18 dark:text-success",
				warning:
					"border-transparent bg-warning/18 text-warning-foreground dark:bg-warning/16 dark:text-warning",
				destructive: "border-transparent bg-destructive/14 text-destructive",
				info: "border-transparent bg-info/14 text-info",
				ghost: "border-border bg-background text-foreground",
				outline: "border-border bg-transparent text-foreground",
			},
			size: {
				default: "h-6",
				sm: "h-5 px-2 text-[11px]",
			},
		},
		defaultVariants: {
			variant: "default",
			size: "default",
		},
	},
);

export interface BadgeProps
	extends React.HTMLAttributes<HTMLDivElement>,
		VariantProps<typeof badgeVariants> {}

function Badge({ className, variant, size, ...props }: BadgeProps) {
	return (
		<div
			className={cn(badgeVariants({ variant, size }), className)}
			{...props}
		/>
	);
}

export { Badge, badgeVariants };

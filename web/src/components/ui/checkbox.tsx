import * as CheckboxPrimitive from "@radix-ui/react-checkbox";
import * as React from "react";

import { cn } from "@/lib/utils";

const Checkbox = React.forwardRef<
	React.ElementRef<typeof CheckboxPrimitive.Root>,
	React.ComponentPropsWithoutRef<typeof CheckboxPrimitive.Root>
>(({ className, children, ...props }, ref) => (
	<CheckboxPrimitive.Root
		ref={ref}
		className={cn(
			"peer group relative inline-flex size-11 shrink-0 items-center justify-center rounded-lg text-primary-foreground outline-none transition-colors before:pointer-events-none before:absolute before:left-1/2 before:top-1/2 before:size-4 before:-translate-x-1/2 before:-translate-y-1/2 before:rounded before:border before:border-input before:bg-background before:shadow-xs before:transition-colors focus-visible:ring-[3px] focus-visible:ring-ring/20 disabled:cursor-not-allowed disabled:opacity-50 data-[state=checked]:before:border-primary data-[state=checked]:before:bg-primary data-[state=indeterminate]:before:border-primary data-[state=indeterminate]:before:bg-primary sm:size-4 sm:rounded",
			className,
		)}
		{...props}
	>
		<CheckboxPrimitive.Indicator className="relative z-10 flex items-center justify-center text-current">
			<svg
				viewBox="0 0 16 16"
				className="size-3.5 group-data-[state=indeterminate]:hidden"
				fill="none"
				aria-hidden="true"
			>
				<path
					d="M3.5 8.5 6.5 11.5 12.5 5.5"
					stroke="currentColor"
					strokeWidth="2"
					strokeLinecap="round"
					strokeLinejoin="round"
				/>
			</svg>
			<svg
				viewBox="0 0 16 16"
				className="hidden size-3.5 group-data-[state=indeterminate]:block"
				fill="none"
				aria-hidden="true"
			>
				<path
					d="M4 8h8"
					stroke="currentColor"
					strokeWidth="2"
					strokeLinecap="round"
				/>
			</svg>
		</CheckboxPrimitive.Indicator>
		{children}
	</CheckboxPrimitive.Root>
));
Checkbox.displayName = CheckboxPrimitive.Root.displayName;

export { Checkbox };

import * as React from "react";

import { cn } from "@/lib/utils";

export interface SwitchProps
	extends Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, "onChange"> {
	checked?: boolean;
	onCheckedChange?: (checked: boolean) => void;
}

const Switch = React.forwardRef<HTMLButtonElement, SwitchProps>(
	(
		{
			checked = false,
			className,
			disabled,
			onCheckedChange,
			onClick,
			...props
		},
		ref,
	) => (
		<button
			{...props}
			ref={ref}
			type="button"
			role="switch"
			aria-checked={checked}
			disabled={disabled}
			data-state={checked ? "checked" : "unchecked"}
			onClick={(event) => {
				onClick?.(event);
				if (!event.defaultPrevented) onCheckedChange?.(!checked);
			}}
			className={cn(
				"inline-flex h-6 w-11 shrink-0 cursor-pointer items-center rounded-full",
				"border-2 border-transparent outline-none transition-colors",
				"motion-reduce:transition-none",
				"focus-visible:ring-[3px] focus-visible:ring-ring/20",
				"disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50",
				checked ? "bg-primary hover:bg-primary/90" : "bg-muted hover:bg-accent",
				className,
			)}
		>
			<span
				aria-hidden="true"
				className={cn(
					"pointer-events-none block size-5 rounded-full bg-background shadow-xs",
					"transition-transform motion-reduce:transition-none",
					checked ? "translate-x-5" : "translate-x-0",
				)}
			/>
		</button>
	),
);
Switch.displayName = "Switch";

export { Switch };

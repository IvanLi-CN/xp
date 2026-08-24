import type { ReactNode } from "react";
import * as React from "react";

import { cn } from "@/lib/utils";

import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "./ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "./ui/tabs";

export type ModuleTabOption = {
	value: string;
	label: string;
};

export function ModuleTabsLayout(props: {
	options: ModuleTabOption[];
	value: string;
	onValueChange: (value: string) => void;
	ariaLabel: string;
	mobileAriaLabel?: string;
	children: ReactNode;
	className?: string;
}) {
	const {
		options,
		value,
		onValueChange,
		ariaLabel,
		mobileAriaLabel = ariaLabel,
		children,
		className,
	} = props;
	const latestValue = React.useRef(value);
	const emitValueChange = React.useCallback(
		(nextValue: string) => {
			if (nextValue === latestValue.current) return;
			latestValue.current = nextValue;
			onValueChange(nextValue);
		},
		[onValueChange],
	);
	React.useEffect(() => {
		latestValue.current = value;
	}, [value]);

	return (
		<Tabs
			className={cn("space-y-3 sm:space-y-3", className)}
			value={value}
			onValueChange={emitValueChange}
		>
			<div className="pb-3 sm:hidden">
				<Select value={value} onValueChange={emitValueChange}>
					<SelectTrigger aria-label={mobileAriaLabel}>
						<SelectValue placeholder={options[0]?.label ?? ariaLabel} />
					</SelectTrigger>
					<SelectContent>
						{options.map((option) => (
							<SelectItem key={option.value} value={option.value}>
								{option.label}
							</SelectItem>
						))}
					</SelectContent>
				</Select>
			</div>
			<div className="hidden max-w-full overflow-x-auto sm:block">
				<TabsList
					className={cn(
						"h-auto max-w-full flex-wrap justify-start gap-1 rounded-2xl",
						"border border-border/70 bg-card p-0.5 shadow-sm sm:p-1",
					)}
					aria-label={ariaLabel}
				>
					{options.map((option) => (
						<TabsTrigger
							key={option.value}
							value={option.value}
							onClick={() => emitValueChange(option.value)}
							className={cn(
								"min-h-11 flex-1 basis-[calc(50%-0.125rem)] whitespace-nowrap",
								"px-2.5 sm:min-h-8 sm:flex-none sm:basis-auto sm:px-3",
							)}
						>
							{option.label}
						</TabsTrigger>
					))}
				</TabsList>
			</div>
			{children}
		</Tabs>
	);
}

export const ModuleTabsPanel = React.forwardRef<
	React.ElementRef<typeof TabsContent>,
	React.ComponentPropsWithoutRef<typeof TabsContent> & {
		keepMounted?: boolean;
	}
>(({ className, keepMounted = false, ...props }, ref) => (
	<TabsContent
		ref={ref}
		forceMount={keepMounted ? true : undefined}
		className={cn(
			"mt-0 outline-none",
			keepMounted && "data-[state=inactive]:hidden",
			className,
		)}
		{...props}
	/>
));
ModuleTabsPanel.displayName = "ModuleTabsPanel";

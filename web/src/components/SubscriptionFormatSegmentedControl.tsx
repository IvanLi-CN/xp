import { useId } from "react";

import {
	SUBSCRIPTION_FORMAT_OPTIONS,
	type SubscriptionFormat,
} from "@/api/subscription";
import { cn } from "@/lib/utils";

type SubscriptionFormatSegmentedControlProps = {
	value: SubscriptionFormat;
	onValueChange: (value: SubscriptionFormat) => void;
	className?: string;
	testId?: string;
};

export function SubscriptionFormatSegmentedControl({
	value,
	onValueChange,
	className,
	testId,
}: SubscriptionFormatSegmentedControlProps) {
	const id = useId();

	return (
		<fieldset className={cn("min-w-0", className)}>
			<legend className="mb-2 text-sm font-medium">Subscription format</legend>
			<div
				aria-label="Subscription format"
				className="grid h-10 w-full grid-cols-3 rounded-full border border-border/70 bg-muted/35 p-1 shadow-xs sm:w-auto"
				data-testid={testId}
				role="radiogroup"
			>
				{SUBSCRIPTION_FORMAT_OPTIONS.map((option) => {
					const checked = option.value === value;
					const optionId = `${id}-${option.value}`;
					return (
						<div className="contents" key={option.value}>
							<input
								checked={checked}
								className="peer sr-only"
								id={optionId}
								name={id}
								onChange={() => onValueChange(option.value)}
								type="radio"
								value={option.value}
							/>
							<label
								className={cn(
									"inline-flex min-w-20 cursor-pointer items-center justify-center rounded-full px-3 text-sm font-medium text-muted-foreground transition-colors",
									"peer-focus-visible:ring-[3px] peer-focus-visible:ring-ring/20",
									checked &&
										"bg-primary text-primary-foreground shadow-sm hover:bg-primary/90",
									!checked && "hover:bg-accent hover:text-accent-foreground",
								)}
								data-state={checked ? "checked" : "unchecked"}
								htmlFor={optionId}
							>
								<span className="truncate">{option.label}</span>
							</label>
						</div>
					);
				})}
			</div>
		</fieldset>
	);
}

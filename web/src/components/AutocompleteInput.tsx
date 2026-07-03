import * as React from "react";

import { Button } from "@/components/Button";
import { Icon } from "@/components/Icon";
import {
	Command,
	CommandEmpty,
	CommandGroup,
	CommandItem,
	CommandList,
} from "@/components/ui/command";
import { Input } from "@/components/ui/input";
import {
	Popover,
	PopoverContent,
	PopoverTrigger,
} from "@/components/ui/popover";
import { cn } from "@/lib/utils";

export type AutocompleteSuggestion = {
	value: string;
	label: string;
	description?: string;
};

type AutocompleteInputProps = Omit<React.ComponentProps<"input">, "list"> & {
	suggestions?: AutocompleteSuggestion[];
	suggestionLabel?: string;
	onSuggestionSelect?: (value: string) => void;
};

const normalize = (value: string) => value.trim().toLowerCase();
const triggerButtonClass = cn(
	"absolute top-1/2 right-1 h-8 min-h-8 w-8 min-w-8",
	"-translate-y-1/2 rounded-lg p-0",
	"text-muted-foreground hover:text-foreground",
	"focus-visible:ring-[3px] focus-visible:ring-ring/20",
);

export const AutocompleteInput = React.forwardRef<
	HTMLInputElement,
	AutocompleteInputProps
>(
	(
		{
			className,
			disabled,
			onKeyDown,
			onSuggestionSelect,
			placeholder,
			suggestionLabel = "Open suggestions",
			suggestions = [],
			value,
			...props
		},
		ref,
	) => {
		const [open, setOpen] = React.useState(false);
		const typedValue = typeof value === "string" ? value : "";
		const query = normalize(typedValue);
		const visibleSuggestions = suggestions.filter((suggestion) => {
			if (query.length === 0) return true;
			const haystack = normalize(
				`${suggestion.value} ${suggestion.label} ${suggestion.description ?? ""}`,
			);
			return haystack.includes(query);
		});
		const hasSuggestions = suggestions.length > 0;

		const selectSuggestion = (selectedValue: string) => {
			onSuggestionSelect?.(selectedValue);
			setOpen(false);
		};

		return (
			<Popover open={open} onOpenChange={setOpen}>
				<div className="relative">
					<Input
						ref={ref}
						value={value}
						disabled={disabled}
						placeholder={placeholder}
						className={cn(hasSuggestions && "pr-12", className)}
						onKeyDown={(event) => {
							if (event.key === "ArrowDown" && hasSuggestions) {
								event.preventDefault();
								setOpen(true);
							}
							onKeyDown?.(event);
						}}
						{...props}
					/>
					{hasSuggestions ? (
						<PopoverTrigger asChild>
							<Button
								type="button"
								variant="ghost"
								size="sm"
								disabled={disabled}
								aria-label={suggestionLabel}
								className={triggerButtonClass}
							>
								<Icon name="tabler:chevron-down" size={16} ariaLabel="" />
							</Button>
						</PopoverTrigger>
					) : null}
				</div>
				{hasSuggestions ? (
					<PopoverContent
						align="end"
						className="w-80 max-w-[calc(100vw-2rem)] p-0 sm:w-[28rem]"
					>
						<Command shouldFilter={false}>
							<CommandList>
								{visibleSuggestions.length === 0 ? (
									<CommandEmpty>No suggestions.</CommandEmpty>
								) : (
									<CommandGroup>
										{visibleSuggestions.map((suggestion) => (
											<CommandItem
												key={suggestion.value}
												value={suggestion.value}
												onSelect={() => selectSuggestion(suggestion.value)}
												className="items-start"
											>
												<span className="flex min-w-0 flex-col gap-0.5">
													<span className="truncate font-mono text-sm">
														{suggestion.value}
													</span>
													<span className="truncate text-xs text-muted-foreground">
														{suggestion.label}
													</span>
													{suggestion.description ? (
														<span className="truncate text-xs text-muted-foreground">
															{suggestion.description}
														</span>
													) : null}
												</span>
											</CommandItem>
										))}
									</CommandGroup>
								)}
							</CommandList>
						</Command>
					</PopoverContent>
				) : null}
			</Popover>
		);
	},
);
AutocompleteInput.displayName = "AutocompleteInput";

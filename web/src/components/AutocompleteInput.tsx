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
	PopoverAnchor,
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
			onChange,
			onClick,
			onFocus,
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
		const openTimerRef = React.useRef<ReturnType<typeof setTimeout> | null>(
			null,
		);
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
		const shouldPromptForEmptyValue = (nextValue: string) =>
			hasSuggestions && normalize(nextValue).length === 0;

		React.useEffect(() => {
			return () => {
				if (openTimerRef.current) clearTimeout(openTimerRef.current);
			};
		}, []);

		const openSuggestionsAfterCurrentInteraction = () => {
			if (openTimerRef.current) clearTimeout(openTimerRef.current);
			openTimerRef.current = setTimeout(() => {
				setOpen(true);
				openTimerRef.current = null;
			}, 0);
		};

		const promptForEmptyValue = (nextValue: string) => {
			if (shouldPromptForEmptyValue(nextValue)) {
				openSuggestionsAfterCurrentInteraction();
			}
		};

		const selectSuggestion = (selectedValue: string) => {
			onSuggestionSelect?.(selectedValue);
			setOpen(false);
		};

		return (
			<Popover open={open} onOpenChange={setOpen}>
				<PopoverAnchor asChild>
					<div className="relative">
						<Input
							ref={ref}
							value={value}
							disabled={disabled}
							placeholder={placeholder}
							className={cn(hasSuggestions && "pr-12", className)}
							onChange={(event) => {
								promptForEmptyValue(event.currentTarget.value);
								onChange?.(event);
							}}
							onClick={(event) => {
								promptForEmptyValue(typedValue);
								onClick?.(event);
							}}
							onFocus={(event) => {
								promptForEmptyValue(typedValue);
								onFocus?.(event);
							}}
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
				</PopoverAnchor>
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
											>
												<span className="min-w-0 truncate font-mono text-sm">
													{suggestion.value}
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

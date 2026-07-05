import { useId, useMemo, useRef, useState } from "react";

import { cn } from "@/lib/utils";

import { Button } from "./Button";
import { Icon } from "./Icon";
import { badgeClass } from "./ui-helpers";
import {
	Command,
	CommandEmpty,
	CommandGroup,
	CommandItem,
	CommandList,
} from "./ui/command";
import { Input } from "./ui/input";
import {
	Popover,
	PopoverAnchor,
	PopoverContent,
	PopoverTrigger,
} from "./ui/popover";

type TagInputProps = {
	label: string;
	value: string[];
	onChange: (next: string[]) => void;
	placeholder?: string;
	helperText?: string;
	disabled?: boolean;
	inputClass?: string;
	validateTag?: (value: string) => string | null;
	allowPrimary?: boolean;
	suggestions?: string[];
	suggestionLabel?: string;
};

function defaultValidateTag(value: string): string | null {
	if (!value) return "Tag is empty.";
	return null;
}

function normalizeToken(token: string): string {
	return token.trim();
}

function splitTokens(text: string): string[] {
	return text
		.split(/[\n\r\t ,]+/g)
		.map((t) => t.trim())
		.filter((t) => t.length > 0);
}

function dedupePreserveOrder(input: string[]): string[] {
	const out: string[] = [];
	const seen = new Set<string>();
	for (const item of input) {
		if (seen.has(item)) continue;
		seen.add(item);
		out.push(item);
	}
	return out;
}

export function TagInput({
	label,
	value,
	onChange,
	placeholder,
	helperText,
	disabled = false,
	inputClass = "xp-input",
	validateTag = defaultValidateTag,
	allowPrimary = true,
	suggestions = [],
	suggestionLabel = "Show suggestions",
}: TagInputProps) {
	const inputId = useId();
	const helperTextId = useId();
	const errorTextId = useId();
	const listboxId = useId();
	const inputRef = useRef<HTMLInputElement | null>(null);

	const [draft, setDraft] = useState("");
	const [error, setError] = useState<string | null>(null);
	const [open, setOpen] = useState(false);
	const [activeSuggestionIndex, setActiveSuggestionIndex] = useState(0);

	const tags = useMemo(
		() =>
			dedupePreserveOrder(
				value.map(normalizeToken).filter((token) => token.length > 0),
			),
		[value],
	);
	const primary = allowPrimary ? (tags[0] ?? "") : "";
	const visibleSuggestions = suggestions.filter((suggestion) => {
		const normalized = normalizeToken(suggestion);
		if (!normalized) return false;
		if (tags.includes(normalized)) return false;
		if (!draft.trim()) return true;
		return normalized.toLowerCase().includes(draft.trim().toLowerCase());
	});
	const hasSuggestions = visibleSuggestions.length > 0;
	const safeActiveSuggestionIndex = Math.min(
		activeSuggestionIndex,
		Math.max(visibleSuggestions.length - 1, 0),
	);
	const activeSuggestion = visibleSuggestions[safeActiveSuggestionIndex];

	function setTags(next: string[]): void {
		onChange(dedupePreserveOrder(next.map(normalizeToken).filter(Boolean)));
	}

	function addManyTokens(rawTokens: string[]): void {
		if (rawTokens.length === 0) return;
		let next = tags.slice();
		let nextError: string | null = null;
		for (const raw of rawTokens) {
			const token = normalizeToken(raw);
			if (!token) continue;
			const validateMessage = validateTag(token);
			if (validateMessage) {
				// Keep best-effort behavior: add valid tokens, surface the first error.
				if (!nextError) nextError = validateMessage;
				continue;
			}
			next.push(token);
		}
		next = dedupePreserveOrder(next);
		setTags(next);
		setError(nextError);
	}

	function addSuggestion(suggestion: string): void {
		addManyTokens([suggestion]);
		setDraft("");
		setOpen(false);
		setActiveSuggestionIndex(0);
		inputRef.current?.focus();
	}

	function removeAt(index: number): void {
		const next = tags.filter((_, i) => i !== index);
		setError(null);
		setTags(next);
	}

	function makePrimaryAt(index: number): void {
		if (!allowPrimary) return;
		if (index <= 0 || index >= tags.length) return;
		const chosen = tags[index];
		const next = [chosen, ...tags.slice(0, index), ...tags.slice(index + 1)];
		setError(null);
		setTags(next);
	}

	function commitDraft(): void {
		const raw = draft;
		setDraft("");
		addManyTokens(splitTokens(raw));
	}

	return (
		<div className="space-y-2">
			<label
				className="block cursor-pointer font-mono text-sm font-medium text-foreground"
				htmlFor={inputId}
			>
				{label}
			</label>

			<div className="space-y-2">
				<Popover open={open && hasSuggestions} onOpenChange={setOpen}>
					<PopoverAnchor asChild>
						<div
							data-testid="tag-input-control"
							className={cn(
								inputClass,
								"flex h-auto min-h-12 w-full flex-wrap items-center gap-2 py-2",
								disabled && "opacity-60",
								error
									? "border-destructive focus-within:border-destructive focus-within:ring-[3px] focus-within:ring-destructive/20"
									: "focus-within:border-ring focus-within:ring-[3px] focus-within:ring-ring/20",
							)}
							onMouseDown={(event) => {
								if (disabled) return;
								// Clicking empty space should focus the input (chips UIs usually behave this way).
								// Do not steal events from action buttons.
								const target = event.target as HTMLElement | null;
								if (target?.closest("button")) return;
								// Do not cancel the default on the actual input; otherwise caret placement/selection breaks.
								if (target?.closest("input")) return;
								event.preventDefault();
								inputRef.current?.focus();
							}}
						>
							{tags.map((tag, idx) => (
								<div key={tag} className="xp-chip-group">
									<span
										className={badgeClass(
											allowPrimary && idx === 0 ? "primary" : "ghost",
											"default",
											"gap-2 font-mono xp-chip-action",
										)}
										title={
											allowPrimary && idx === 0
												? "Primary (used for dest / probe)"
												: tag
										}
									>
										{allowPrimary && idx === 0 ? (
											<Icon
												name="tabler:star-filled"
												size={14}
												ariaLabel="Primary"
											/>
										) : null}
										<span>{tag}</span>
									</span>

									{allowPrimary && idx !== 0 ? (
										<Button
											type="button"
											variant="ghost"
											size="sm"
											className="h-7 px-2 xp-chip-action"
											onClick={() => makePrimaryAt(idx)}
											disabled={disabled}
											title="Make primary"
										>
											<Icon
												name="tabler:star"
												size={14}
												ariaLabel="Make primary"
											/>
										</Button>
									) : null}

									<Button
										type="button"
										variant="ghost"
										size="sm"
										className="h-7 px-2 xp-chip-action"
										onClick={() => removeAt(idx)}
										disabled={disabled}
										title="Remove"
									>
										<Icon name="tabler:x" size={14} ariaLabel="Remove" />
									</Button>
								</div>
							))}

							<div className="flex min-w-[16ch] grow items-center gap-2">
								<Input
									ref={inputRef}
									type="text"
									className={cn(
										"h-auto min-w-0 grow border-0 bg-transparent px-0 py-0",
										"font-mono text-sm shadow-none",
										"focus-visible:border-transparent focus-visible:ring-0",
										disabled && "opacity-60",
									)}
									id={inputId}
									value={draft}
									placeholder={placeholder}
									disabled={disabled}
									aria-label={label}
									aria-invalid={error ? true : undefined}
									aria-describedby={
										error ? `${helperTextId} ${errorTextId}` : helperTextId
									}
									aria-autocomplete={
										suggestions.length > 0 ? "list" : undefined
									}
									aria-controls={open && hasSuggestions ? listboxId : undefined}
									aria-expanded={suggestions.length > 0 ? open : undefined}
									aria-activedescendant={
										open && activeSuggestion
											? `${listboxId}-option-${safeActiveSuggestionIndex}`
											: undefined
									}
									onChange={(event) => {
										setDraft(event.target.value);
										setActiveSuggestionIndex(0);
										setOpen(true);
										if (error) setError(null);
									}}
									onFocus={() => {
										setOpen(true);
										setActiveSuggestionIndex(0);
									}}
									onClick={() => {
										setOpen(true);
										setActiveSuggestionIndex(0);
									}}
									onKeyDown={(event) => {
										if (event.key === "ArrowDown" && hasSuggestions) {
											event.preventDefault();
											setOpen(true);
											setActiveSuggestionIndex((currentIndex) =>
												Math.min(
													currentIndex + 1,
													visibleSuggestions.length - 1,
												),
											);
											return;
										}
										if (event.key === "ArrowUp" && open && hasSuggestions) {
											event.preventDefault();
											setActiveSuggestionIndex((currentIndex) =>
												Math.max(currentIndex - 1, 0),
											);
											return;
										}
										if (event.key === "Enter" && open && activeSuggestion) {
											event.preventDefault();
											addSuggestion(activeSuggestion);
											return;
										}
										if (event.key === "Enter" || event.key === ",") {
											event.preventDefault();
											commitDraft();
											return;
										}
										if (event.key === "Escape" && open) {
											event.preventDefault();
											setOpen(false);
											return;
										}
										if (
											event.key === "Backspace" &&
											draft.length === 0 &&
											tags.length > 0
										) {
											event.preventDefault();
											removeAt(tags.length - 1);
										}
									}}
									onPaste={(event) => {
										const text = event.clipboardData?.getData("text") ?? "";
										const tokens = splitTokens(text);
										if (tokens.length >= 2) {
											event.preventDefault();
											addManyTokens(tokens);
											setDraft("");
										}
									}}
								/>

								{suggestions.length > 0 ? (
									<PopoverTrigger asChild>
										<Button
											type="button"
											variant="ghost"
											size="sm"
											className="size-8 shrink-0 px-0"
											disabled={disabled}
											aria-label={suggestionLabel}
											title={suggestionLabel}
										>
											<Icon name="tabler:chevron-down" size={16} ariaLabel="" />
										</Button>
									</PopoverTrigger>
								) : null}

								<Button
									type="button"
									variant="ghost"
									size="sm"
									className="size-8 shrink-0 px-0"
									onClick={() => commitDraft()}
									disabled={disabled || draft.trim().length === 0}
									aria-label="Add"
									title="Add"
								>
									<Icon name="tabler:plus" size={16} ariaLabel="Add" />
								</Button>
							</div>
						</div>
					</PopoverAnchor>
					<PopoverContent
						align="start"
						data-testid="tag-input-suggestions"
						className="w-[var(--radix-popper-anchor-width)] max-w-[calc(100vw-2rem)] p-0"
						onOpenAutoFocus={(event) => event.preventDefault()}
					>
						<Command shouldFilter={false}>
							<CommandList id={listboxId}>
								{visibleSuggestions.length === 0 ? (
									<CommandEmpty>No suggestions.</CommandEmpty>
								) : (
									<CommandGroup>
										{visibleSuggestions.map((suggestion, index) => (
											<CommandItem
												id={`${listboxId}-option-${index}`}
												key={suggestion}
												value={suggestion}
												data-active={index === safeActiveSuggestionIndex}
												onSelect={() => addSuggestion(suggestion)}
												className="data-[active=true]:bg-accent data-[active=true]:text-accent-foreground"
											>
												<span className="min-w-0 truncate font-mono text-sm">
													{suggestion}
												</span>
											</CommandItem>
										))}
									</CommandGroup>
								)}
							</CommandList>
						</Command>
					</PopoverContent>
				</Popover>

				<p className="text-xs text-muted-foreground" id={helperTextId}>
					{helperText ? helperText : null}
					{primary ? (
						<span className="ml-2 font-mono opacity-70">
							(primary={primary})
						</span>
					) : null}
				</p>

				{error ? (
					<p className="text-xs text-destructive" role="alert" id={errorTextId}>
						{error}
					</p>
				) : null}
			</div>
		</div>
	);
}

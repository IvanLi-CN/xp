import { useId, useMemo, useRef, useState } from "react";

import { Icon } from "./Icon";

type TagInputProps = {
	label: string;
	value: string[];
	onChange: (next: string[]) => void;
	placeholder?: string;
	helperText?: string;
	disabled?: boolean;
	inputClass?: string;
	validateTag?: (value: string) => string | null;
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
	inputClass = "input input-bordered",
	validateTag = defaultValidateTag,
}: TagInputProps) {
	const inputId = useId();
	const helperTextId = useId();
	const errorTextId = useId();
	const inputRef = useRef<HTMLInputElement | null>(null);

	const [draft, setDraft] = useState("");
	const [error, setError] = useState<string | null>(null);

	const tags = useMemo(
		() =>
			dedupePreserveOrder(
				value.map(normalizeToken).filter((token) => token.length > 0),
			),
		[value],
	);
	const primary = tags[0] ?? "";

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

	function removeAt(index: number): void {
		const next = tags.filter((_, i) => i !== index);
		setError(null);
		setTags(next);
	}

	function makePrimaryAt(index: number): void {
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
		<div className="form-control">
			<div className="label">
				<label
					className="label-text font-mono cursor-pointer"
					htmlFor={inputId}
				>
					{label}
				</label>
			</div>

			<div className="space-y-2">
				<div
					className={[
						// A custom tag input that looks/behaves like a single "input" control.
						// We reuse DaisyUI's input classes passed from the page so density is consistent.
						inputClass,
						"w-full !h-auto min-h-12 flex flex-wrap items-center gap-2 py-2",
						disabled ? "opacity-60" : null,
						error ? "input-error" : null,
						// Focus ring to match other inputs.
						"focus-within:outline focus-within:outline-2 focus-within:outline-primary/30",
					].join(" ")}
					onMouseDown={(event) => {
						if (disabled) return;
						// Clicking empty space should focus the input (chips UIs usually behave this way).
						// Do not steal events from action buttons.
						const target = event.target as HTMLElement | null;
						if (target?.closest("button")) return;
						event.preventDefault();
						inputRef.current?.focus();
					}}
				>
					{tags.map((tag, idx) => (
						<div key={tag} className="join">
							<span
								className={[
									"badge join-item gap-2 font-mono",
									idx === 0 ? "badge-primary" : "badge-ghost",
								].join(" ")}
								title={idx === 0 ? "Primary (used for dest / probe)" : tag}
							>
								{idx === 0 ? (
									<Icon
										name="tabler:star-filled"
										size={14}
										ariaLabel="Primary"
									/>
								) : null}
								<span>{tag}</span>
							</span>

							{idx !== 0 ? (
								<button
									type="button"
									className="btn join-item btn-ghost btn-xs"
									onClick={() => makePrimaryAt(idx)}
									disabled={disabled}
									title="Make primary"
								>
									<Icon name="tabler:star" size={14} ariaLabel="Make primary" />
								</button>
							) : null}

							<button
								type="button"
								className="btn join-item btn-ghost btn-xs"
								onClick={() => removeAt(idx)}
								disabled={disabled}
								title="Remove"
							>
								<Icon name="tabler:x" size={14} ariaLabel="Remove" />
							</button>
						</div>
					))}

					<div className="flex min-w-[16ch] grow items-center gap-2">
						<input
							ref={inputRef}
							type="text"
							className={[
								// Keep visual parity with other fields while allowing chips to wrap.
								"grow bg-transparent font-mono outline-none",
								disabled ? "opacity-60" : null,
							]
								.filter(Boolean)
								.join(" ")}
							id={inputId}
							value={draft}
							placeholder={placeholder}
							disabled={disabled}
							aria-label={label}
							aria-invalid={error ? true : undefined}
							aria-describedby={
								error ? `${helperTextId} ${errorTextId}` : helperTextId
							}
							onChange={(event) => {
								setDraft(event.target.value);
								if (error) setError(null);
							}}
							onKeyDown={(event) => {
								if (event.key === "Enter" || event.key === ",") {
									event.preventDefault();
									commitDraft();
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

						<button
							type="button"
							className="btn btn-ghost btn-sm btn-square"
							onClick={() => commitDraft()}
							disabled={disabled || draft.trim().length === 0}
							aria-label="Add"
							title="Add"
						>
							<Icon name="tabler:plus" size={16} ariaLabel="Add" />
						</button>
					</div>
				</div>

				<p className="text-xs opacity-70" id={helperTextId}>
					{helperText ? helperText : null}
					{primary ? (
						<span className="ml-2 font-mono opacity-70">
							(primary={primary})
						</span>
					) : null}
				</p>

				{error ? (
					<p className="text-xs text-error" role="alert" id={errorTextId}>
						{error}
					</p>
				) : null}
			</div>
		</div>
	);
}

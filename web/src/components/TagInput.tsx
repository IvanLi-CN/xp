import { useMemo, useState } from "react";

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
		<label className="form-control">
			<div className="label">
				<span className="label-text font-mono">{label}</span>
			</div>

			<div className="space-y-2">
				<div className="flex flex-wrap gap-2">
					{tags.map((tag, idx) => (
						<div key={tag} className="flex items-center gap-1">
							<span
								className={[
									"badge gap-2 font-mono",
									idx === 0 ? "badge-primary" : "badge-ghost",
								].join(" ")}
								title={idx === 0 ? "Primary (used for dest / probe)" : tag}
							>
								<span>{tag}</span>
								{idx === 0 ? <span className="opacity-80">primary</span> : null}
							</span>

							{idx !== 0 ? (
								<button
									type="button"
									className="btn btn-ghost btn-xs"
									onClick={() => makePrimaryAt(idx)}
									disabled={disabled}
									title="Make primary"
								>
									<Icon name="tabler:star" size={14} ariaLabel="Make primary" />
								</button>
							) : null}

							<button
								type="button"
								className="btn btn-ghost btn-xs"
								onClick={() => removeAt(idx)}
								disabled={disabled}
								title="Remove"
							>
								<Icon name="tabler:x" size={14} ariaLabel="Remove" />
							</button>
						</div>
					))}
				</div>

				<div className="flex items-center gap-2">
					<input
						type="text"
						className={inputClass}
						value={draft}
						placeholder={placeholder}
						disabled={disabled}
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
						className="btn btn-secondary btn-sm"
						onClick={() => commitDraft()}
						disabled={disabled || draft.trim().length === 0}
						title="Add"
					>
						Add
					</button>
				</div>

				<p className="text-xs opacity-70">
					{helperText ? helperText : null}
					{primary ? (
						<span className="ml-2 font-mono opacity-70">
							(primary={primary})
						</span>
					) : null}
				</p>

				{error ? (
					<p className="text-xs text-error" role="alert">
						{error}
					</p>
				) : null}
			</div>
		</label>
	);
}

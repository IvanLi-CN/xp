import {
	useCallback,
	useEffect,
	useLayoutEffect,
	useMemo,
	useRef,
	useState,
} from "react";
import { createPortal } from "react-dom";

import {
	formatQuotaBytesCompactInput,
	formatQuotaBytesHuman,
	parseQuotaInputToBytes,
} from "../utils/quota";
import { Button } from "./Button";

export type NodeQuotaEditorValue = number | "mixed";

function isInDom(
	target: EventTarget | null,
	container: HTMLElement | null,
): boolean {
	if (!target || !container) return false;
	if (!(target instanceof Node)) return false;
	return container.contains(target);
}

function ErrorPopover(props: {
	anchorRect: DOMRect;
	message: string;
	id: string;
}) {
	const { anchorRect, message, id } = props;
	const margin = 12;
	const left = Math.max(margin, anchorRect.left);
	const top = anchorRect.bottom + 8;
	const maxWidth = Math.max(160, window.innerWidth - left - margin);

	return createPortal(
		<div
			id={id}
			role="alert"
			className="rounded-md border border-rose-300 bg-rose-50 px-3 py-2 text-xs text-rose-900 shadow-md"
			style={{
				position: "fixed",
				left,
				top,
				zIndex: 1000,
				maxWidth,
				width: "fit-content",
				whiteSpace: "normal",
			}}
		>
			{message}
		</div>,
		document.body,
	);
}

export function NodeQuotaEditor(props: {
	value: NodeQuotaEditorValue;
	disabled?: boolean;
	onApply: (nextBytes: number) => Promise<void>;
}) {
	const { value, disabled = false, onApply } = props;

	const [isEditing, setIsEditing] = useState(false);
	const [draft, setDraft] = useState("");
	const [error, setError] = useState<string | null>(null);
	const [anchorRect, setAnchorRect] = useState<DOMRect | null>(null);
	const [isSaving, setIsSaving] = useState(false);

	const containerRef = useRef<HTMLDivElement | null>(null);
	const inputRef = useRef<HTMLInputElement | null>(null);
	const popoverId = useMemo(
		() => `quota-error-${Math.random().toString(16).slice(2)}`,
		[],
	);

	const display = value === "mixed" ? "Mixed" : formatQuotaBytesHuman(value);
	const editableDefault =
		value === "mixed" ? "" : formatQuotaBytesCompactInput(value);

	const cancel = useCallback(() => {
		setIsEditing(false);
		setError(null);
		setAnchorRect(null);
		setDraft(editableDefault);
	}, [editableDefault]);

	useEffect(() => {
		if (!isEditing) return;
		setDraft(editableDefault);
		setError(null);
		setAnchorRect(null);
	}, [editableDefault, isEditing]);

	useEffect(() => {
		if (!isEditing) return;

		function onPointerDown(event: PointerEvent) {
			const target = event.target;
			const popover = document.getElementById(popoverId);
			if (isInDom(target, containerRef.current)) return;
			if (isInDom(target, popover)) return;
			cancel();
		}

		document.addEventListener("pointerdown", onPointerDown, true);
		return () =>
			document.removeEventListener("pointerdown", onPointerDown, true);
	}, [cancel, isEditing, popoverId]);

	useLayoutEffect(() => {
		if (!isEditing) return;
		if (!inputRef.current) return;
		inputRef.current.focus();
		inputRef.current.select();
	}, [isEditing]);

	useLayoutEffect(() => {
		if (!isEditing || !error) return;

		const input = inputRef.current;
		if (!input) return;

		input.scrollIntoView({ block: "center", inline: "nearest" });

		const update = () => setAnchorRect(input.getBoundingClientRect());

		// First measurement after scroll.
		requestAnimationFrame(update);

		window.addEventListener("scroll", update, true);
		window.addEventListener("resize", update);
		return () => {
			window.removeEventListener("scroll", update, true);
			window.removeEventListener("resize", update);
		};
	}, [error, isEditing]);

	async function apply() {
		if (disabled || isSaving) return;
		const parsed = parseQuotaInputToBytes(draft);
		if (!parsed.ok) {
			setError(parsed.error);
			return;
		}

		setError(null);
		setIsSaving(true);
		try {
			await onApply(parsed.bytes);
			setIsEditing(false);
			setAnchorRect(null);
		} catch (err) {
			setError(err instanceof Error ? err.message : String(err));
		} finally {
			setIsSaving(false);
		}
	}

	return (
		<div ref={containerRef} className="mt-2">
			{isEditing ? (
				<div className="flex flex-wrap items-center gap-2">
					<div className="relative">
						<input
							ref={inputRef}
							className={[
								"input input-bordered input-xs font-mono",
								error ? "input-error" : "",
							]
								.filter(Boolean)
								.join(" ")}
							value={draft}
							disabled={disabled || isSaving}
							aria-invalid={Boolean(error)}
							aria-describedby={error ? popoverId : undefined}
							placeholder={value === "mixed" ? "e.g. 10GiB" : undefined}
							onChange={(event) => {
								setDraft(event.target.value);
								setError(null);
							}}
							onKeyDown={(event) => {
								if (event.key === "Escape") {
									event.preventDefault();
									cancel();
								}
								if (event.key === "Enter") {
									event.preventDefault();
									void apply();
								}
							}}
						/>
						{error && anchorRect ? (
							<ErrorPopover
								anchorRect={anchorRect}
								message={error}
								id={popoverId}
							/>
						) : null}
					</div>
					<Button
						size="sm"
						loading={isSaving}
						disabled={disabled || isSaving}
						onClick={() => void apply()}
					>
						Apply
					</Button>
					<Button
						size="sm"
						variant="ghost"
						disabled={disabled || isSaving}
						onClick={cancel}
					>
						Cancel
					</Button>
				</div>
			) : (
				<button
					type="button"
					className="btn btn-ghost btn-xs px-2"
					disabled={disabled}
					onClick={() => setIsEditing(true)}
				>
					<span className="font-mono text-xs opacity-70">Quota: {display}</span>
					<span className="font-mono text-[10px] opacity-50">(edit)</span>
				</button>
			)}
		</div>
	);
}

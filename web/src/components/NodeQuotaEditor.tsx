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

function clamp(n: number, min: number, max: number): number {
	return Math.min(max, Math.max(min, n));
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
	const [isSaving, setIsSaving] = useState(false);
	const [popoverPos, setPopoverPos] = useState<{
		left: number;
		top: number;
	} | null>(null);

	const containerRef = useRef<HTMLDivElement | null>(null);
	const triggerRef = useRef<HTMLButtonElement | null>(null);
	const editorRef = useRef<HTMLDialogElement | null>(null);
	const inputRef = useRef<HTMLInputElement | null>(null);
	const editorId = useMemo(
		() => `quota-editor-${Math.random().toString(16).slice(2)}`,
		[],
	);

	const display = value === "mixed" ? "Mixed" : formatQuotaBytesHuman(value);
	const editableDefault =
		value === "mixed" ? "" : formatQuotaBytesCompactInput(value);

	const updatePopoverPosition = useCallback(() => {
		const trigger = triggerRef.current;
		const editor = editorRef.current;
		if (!trigger || !editor) return;

		const margin = 12;
		const offset = 8;
		const anchor = trigger.getBoundingClientRect();
		const editorRect = editor.getBoundingClientRect();

		const canPlaceBelow =
			anchor.bottom + offset + editorRect.height + margin <= window.innerHeight;
		const desiredTop = canPlaceBelow
			? anchor.bottom + offset
			: anchor.top - offset - editorRect.height;
		const top = clamp(
			desiredTop,
			margin,
			window.innerHeight - editorRect.height - margin,
		);

		const desiredLeft = anchor.left + anchor.width / 2 - editorRect.width / 2;
		const left = clamp(
			desiredLeft,
			margin,
			window.innerWidth - editorRect.width - margin,
		);

		setPopoverPos({ left, top });
	}, []);

	const cancel = useCallback(() => {
		setIsEditing(false);
		setError(null);
		setPopoverPos(null);
		setDraft(editableDefault);
	}, [editableDefault]);

	useEffect(() => {
		if (!isEditing) return;
		setDraft(editableDefault);
		setError(null);
		setPopoverPos(null);
		requestAnimationFrame(() => updatePopoverPosition());
	}, [editableDefault, isEditing, updatePopoverPosition]);

	useEffect(() => {
		if (!isEditing) return;

		function onPointerDown(event: PointerEvent) {
			const target = event.target;
			if (isInDom(target, containerRef.current)) return;
			if (isInDom(target, editorRef.current)) return;
			cancel();
		}

		document.addEventListener("pointerdown", onPointerDown, true);
		return () =>
			document.removeEventListener("pointerdown", onPointerDown, true);
	}, [cancel, isEditing]);

	useLayoutEffect(() => {
		if (!isEditing) return;
		if (!inputRef.current) return;
		inputRef.current.focus();
		inputRef.current.select();
	}, [isEditing]);

	useLayoutEffect(() => {
		if (!isEditing) return;

		const trigger = triggerRef.current;
		if (!trigger) return;

		trigger.scrollIntoView({ block: "center", inline: "nearest" });

		const update = () => updatePopoverPosition();

		requestAnimationFrame(() => updatePopoverPosition());
		requestAnimationFrame(() => updatePopoverPosition());

		window.addEventListener("scroll", update, true);
		window.addEventListener("resize", update);
		return () => {
			window.removeEventListener("scroll", update, true);
			window.removeEventListener("resize", update);
		};
	}, [isEditing, updatePopoverPosition]);

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
			setPopoverPos(null);
		} catch (err) {
			setError(err instanceof Error ? err.message : String(err));
		} finally {
			setIsSaving(false);
		}
	}

	const editorPopover = isEditing
		? createPortal(
				(() => {
					const margin = 12;
					const desiredWidth = 260;
					const left = popoverPos ? popoverPos.left : margin;
					const top = popoverPos ? popoverPos.top : margin;
					const maxWidth = Math.max(220, window.innerWidth - left - margin);

					return (
						<dialog
							ref={editorRef}
							id={editorId}
							aria-label="Edit node quota"
							className="rounded-xl border border-base-300 bg-base-100 p-3 shadow-lg"
							open
							onCancel={(event) => {
								event.preventDefault();
								cancel();
							}}
							onClose={() => cancel()}
							style={{
								position: "fixed",
								left,
								top,
								zIndex: 1000,
								width: desiredWidth,
								maxWidth,
								visibility: popoverPos ? "visible" : "hidden",
								margin: 0,
							}}
						>
							<div className="flex flex-col gap-2">
								<input
									ref={inputRef}
									className={[
										"input input-bordered input-sm w-full font-mono",
										error ? "input-error" : "",
									]
										.filter(Boolean)
										.join(" ")}
									value={draft}
									disabled={disabled || isSaving}
									aria-invalid={Boolean(error)}
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

								{error ? (
									<div className="text-xs text-error">{error}</div>
								) : null}

								<div className="flex items-center justify-end gap-2">
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
							</div>
						</dialog>
					);
				})(),
				document.body,
			)
		: null;

	return (
		<div ref={containerRef} className="mt-2">
			<button
				ref={triggerRef}
				type="button"
				className="btn btn-ghost btn-xs px-2"
				disabled={disabled}
				aria-expanded={isEditing}
				aria-controls={isEditing ? editorId : undefined}
				onClick={() => {
					if (disabled) return;
					setIsEditing(true);
					setPopoverPos(null);
					requestAnimationFrame(() => updatePopoverPosition());
				}}
			>
				<span className="font-mono text-xs opacity-70">Quota: {display}</span>
				<span className="font-mono text-[10px] opacity-50">(edit)</span>
			</button>
			{editorPopover}
		</div>
	);
}

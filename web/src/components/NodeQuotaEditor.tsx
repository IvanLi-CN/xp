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
	const [anchorRect, setAnchorRect] = useState<DOMRect | null>(null);
	const [isSaving, setIsSaving] = useState(false);

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

	const measureAnchor = useCallback(() => {
		const trigger = triggerRef.current;
		if (!trigger) return;
		setAnchorRect(trigger.getBoundingClientRect());
	}, []);

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
		measureAnchor();
	}, [editableDefault, isEditing, measureAnchor]);

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

		const update = () => measureAnchor();

		requestAnimationFrame(update);

		window.addEventListener("scroll", update, true);
		window.addEventListener("resize", update);
		return () => {
			window.removeEventListener("scroll", update, true);
			window.removeEventListener("resize", update);
		};
	}, [isEditing, measureAnchor]);

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

	const editorPopover =
		isEditing && anchorRect
			? createPortal(
					(() => {
						const margin = 12;
						const desiredWidth = 320;
						const left = clamp(
							anchorRect.left,
							margin,
							window.innerWidth - desiredWidth - margin,
						);
						const top = anchorRect.bottom + 8;
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
								}}
							>
								<div className="flex items-start gap-2">
									<div className="w-full">
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
											<div className="mt-2 text-xs text-error">{error}</div>
										) : null}
									</div>

									<div className="flex flex-col gap-2">
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
					measureAnchor();
				}}
			>
				<span className="font-mono text-xs opacity-70">Quota: {display}</span>
				<span className="font-mono text-[10px] opacity-50">(edit)</span>
			</button>
			{editorPopover}
		</div>
	);
}

import { useEffect, useMemo, useRef, useState } from "react";

import { cn } from "@/lib/utils";

import { Icon } from "./Icon";

type EditorShortcutPlatform = "auto" | "mac" | "windows";

function useIsMacPlatform(): boolean {
	return useMemo(() => {
		if (typeof navigator === "undefined") return false;
		const ua = `${navigator.userAgent} ${navigator.platform}`.toLowerCase();
		return ua.includes("mac") || ua.includes("iphone") || ua.includes("ipad");
	}, []);
}

export function useEditorShortcutItems(
	platform: EditorShortcutPlatform = "auto",
): Array<{
	label: string;
	combos: string[][];
}> {
	const autoIsMac = useIsMacPlatform();
	const isMac = platform === "auto" ? autoIsMac : platform === "mac";

	return useMemo(
		() => [
			{
				label: "Search",
				combos: [isMac ? ["⌘", "F"] : ["Ctrl", "F"]],
			},
			{
				label: "Fold",
				combos: [
					isMac ? ["⌘", "⌥", "["] : ["Ctrl", "Shift", "["],
					isMac ? ["⌃", "⌥", "["] : ["Ctrl", "Alt", "["],
				],
			},
			{
				label: "Unfold",
				combos: [
					isMac ? ["⌘", "⌥", "]"] : ["Ctrl", "Shift", "]"],
					isMac ? ["⌃", "⌥", "]"] : ["Ctrl", "Alt", "]"],
				],
			},
		],
		[isMac],
	);
}

type EditorShortcutHintProps = {
	platform?: EditorShortcutPlatform;
};

function ShortcutCombo({ combo }: { combo: string[] }) {
	return (
		<span className="inline-flex items-center gap-1">
			{combo.map((key) => (
				<kbd
					key={key}
					className="inline-flex h-6 min-w-7 items-center justify-center rounded border border-border bg-muted px-1.5 font-mono text-[10px] font-semibold tracking-tight text-foreground shadow-xs"
				>
					{key}
				</kbd>
			))}
		</span>
	);
}

function ShortcutInlinePreview({
	shortcuts,
	innerRef,
}: {
	shortcuts: Array<{ label: string; combos: string[][] }>;
	innerRef?: React.Ref<HTMLDivElement>;
}) {
	return (
		<div
			ref={innerRef}
			className="flex min-w-0 items-center gap-3 overflow-hidden whitespace-nowrap"
		>
			{shortcuts.map((shortcut) => (
				<div
					key={shortcut.label}
					className="inline-flex shrink-0 items-center gap-1.5"
				>
					<span>{shortcut.label}</span>
					<ShortcutCombo combo={shortcut.combos[0] ?? []} />
				</div>
			))}
		</div>
	);
}

export function EditorShortcutHint({
	platform = "auto",
}: EditorShortcutHintProps) {
	const shortcuts = useEditorShortcutItems(platform);
	const [open, setOpen] = useState(false);
	const [overflowing, setOverflowing] = useState(false);
	const hoverTimerRef = useRef<number | null>(null);
	const closeTimerRef = useRef<number | null>(null);
	const panelRef = useRef<HTMLDivElement | null>(null);
	const rowRef = useRef<HTMLDivElement | null>(null);
	const previewRef = useRef<HTMLDivElement | null>(null);

	useEffect(
		() => () => {
			if (hoverTimerRef.current !== null) {
				window.clearTimeout(hoverTimerRef.current);
			}
			if (closeTimerRef.current !== null) {
				window.clearTimeout(closeTimerRef.current);
			}
		},
		[],
	);

	useEffect(() => {
		const row = rowRef.current;
		const preview = previewRef.current;
		if (!row || !preview) return;

		const updateOverflow = () => {
			setOverflowing(preview.scrollWidth > preview.clientWidth + 1);
		};

		updateOverflow();

		if (typeof ResizeObserver === "undefined") {
			window.addEventListener("resize", updateOverflow);
			return () => window.removeEventListener("resize", updateOverflow);
		}

		const observer = new ResizeObserver(updateOverflow);
		observer.observe(row);
		observer.observe(preview);
		return () => observer.disconnect();
	}, []);

	useEffect(() => {
		if (!overflowing && open) {
			setOpen(false);
		}
	}, [open, overflowing]);

	const openSoon = () => {
		if (!overflowing) return;
		if (closeTimerRef.current !== null) {
			window.clearTimeout(closeTimerRef.current);
			closeTimerRef.current = null;
		}
		if (open) return;
		if (hoverTimerRef.current !== null) return;
		hoverTimerRef.current = window.setTimeout(() => {
			setOpen(true);
			hoverTimerRef.current = null;
		}, 500);
	};

	const openNow = () => {
		if (!overflowing) return;
		if (hoverTimerRef.current !== null) {
			window.clearTimeout(hoverTimerRef.current);
			hoverTimerRef.current = null;
		}
		if (closeTimerRef.current !== null) {
			window.clearTimeout(closeTimerRef.current);
			closeTimerRef.current = null;
		}
		setOpen(true);
	};

	const scheduleClose = () => {
		if (hoverTimerRef.current !== null) {
			window.clearTimeout(hoverTimerRef.current);
			hoverTimerRef.current = null;
		}
		if (closeTimerRef.current !== null) {
			window.clearTimeout(closeTimerRef.current);
		}
		closeTimerRef.current = window.setTimeout(() => {
			setOpen(false);
			closeTimerRef.current = null;
		}, 260);
	};

	return (
		<div className="relative w-full">
			<div
				ref={rowRef}
				className={cn(
					"flex h-9 w-full items-center justify-between gap-3 overflow-hidden rounded-xl border border-border bg-muted/25 px-3 text-left text-[11px] leading-5 text-muted-foreground shadow-xs",
					open ? "rounded-b-none" : "",
				)}
				onMouseEnter={openSoon}
				onMouseLeave={scheduleClose}
			>
				<div className="flex min-w-0 items-center gap-3 overflow-hidden">
					<div className="flex shrink-0 items-center gap-1.5">
						<Icon name="tabler:keyboard" size={14} />
						<span className="font-medium text-foreground/80">Shortcuts</span>
					</div>
					<ShortcutInlinePreview innerRef={previewRef} shortcuts={shortcuts} />
				</div>
				{overflowing ? (
					<button
						type="button"
						aria-expanded={open}
						aria-controls="editor-shortcut-panel"
						aria-label={open ? "Collapse shortcuts" : "Expand shortcuts"}
						className="inline-flex size-6 shrink-0 items-center justify-center rounded-md border border-border bg-background text-foreground shadow-xs"
						onBlur={scheduleClose}
						onClick={() => {
							setOpen((prev) => !prev);
						}}
					>
						<Icon
							name={open ? "tabler:chevron-up" : "tabler:chevron-down"}
							size={14}
						/>
					</button>
				) : null}
			</div>
			{overflowing ? (
				<div
					id="editor-shortcut-panel"
					ref={panelRef}
					className={cn(
						"absolute left-0 right-0 top-[calc(100%-1px)] z-20 overflow-hidden rounded-b-xl border border-border border-t-0 bg-background px-3 py-2 shadow-lg",
						open ? "block" : "hidden",
					)}
					onMouseEnter={openNow}
					onMouseLeave={scheduleClose}
				>
					<div className="flex flex-wrap items-center gap-x-4 gap-y-2 text-[11px] leading-5 text-muted-foreground">
						{shortcuts.map((shortcut) => (
							<div
								key={shortcut.label}
								className="inline-flex items-center gap-2"
							>
								<span className="whitespace-nowrap">{shortcut.label}</span>
								<div className="flex flex-wrap items-center gap-1">
									{shortcut.combos.map((combo, index) => (
										<div
											key={`${shortcut.label}-${String(index)}`}
											className="inline-flex items-center gap-1"
										>
											{index > 0 ? (
												<span className="text-muted-foreground/60">/</span>
											) : null}
											<ShortcutCombo combo={combo} />
										</div>
									))}
								</div>
							</div>
						))}
					</div>
				</div>
			) : null}
		</div>
	);
}

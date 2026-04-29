import type { PointerEvent as ReactPointerEvent } from "react";
import { useEffect, useId, useMemo, useRef, useState } from "react";

import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

import type { SubscriptionFormat } from "../api/subscription";
import { Icon } from "./Icon";

type SubscriptionPreviewDialogProps = {
	open: boolean;
	onClose: () => void;
	subscriptionUrl: string;
	format: SubscriptionFormat;
	loading: boolean;
	content: string;
	error?: string | null;
};

type CodeLanguage = "yaml" | "json" | "text";

type ClashFields = {
	servername?: string;
	publicKey?: string;
	shortId?: string;
};

function truncateMiddle(value: string, head: number, tail: number): string {
	if (value.length <= head + tail + 1) return value;
	return `${value.slice(0, head)}…${value.slice(-tail)}`;
}

function isProbablyJson(text: string): boolean {
	const trimmed = text.trim();
	if (!(trimmed.startsWith("{") || trimmed.startsWith("["))) return false;
	try {
		JSON.parse(trimmed);
		return true;
	} catch {
		return false;
	}
}

function chooseLanguage(
	format: SubscriptionFormat,
	content: string,
): CodeLanguage {
	if (
		format === "clash" ||
		format === "mihomo" ||
		format === "mihomo_legacy" ||
		format === "mihomo_provider"
	) {
		return "yaml";
	}
	if (isProbablyJson(content)) return "json";
	return "text";
}

function parseYamlScalar(raw: string): string {
	const trimmed = raw.trim();
	if (
		(trimmed.startsWith('"') && trimmed.endsWith('"')) ||
		(trimmed.startsWith("'") && trimmed.endsWith("'"))
	) {
		return trimmed.slice(1, -1);
	}
	return trimmed;
}

function extractClashFields(text: string): ClashFields {
	const lines = text.split("\n");

	let servername: string | undefined;
	let publicKey: string | undefined;
	let shortId: string | undefined;

	let inRealityOpts = false;
	let realityIndent = 0;

	for (const line of lines) {
		const raw = line;
		const trimmed = raw.trim();
		if (!trimmed || trimmed.startsWith("#")) continue;

		const indent = raw.length - raw.trimStart().length;

		if (inRealityOpts && indent <= realityIndent) {
			inRealityOpts = false;
		}

		const maybePair = trimmed.startsWith("- ")
			? trimmed.slice(2).trimStart()
			: trimmed;

		const match = maybePair.match(/^([A-Za-z0-9_-]+)\s*:\s*(.*)$/);
		if (!match) continue;

		const key = match[1];
		const value = match[2] ?? "";

		if (!servername && key === "servername") {
			servername = parseYamlScalar(value);
		}

		if (key === "reality-opts" && value.length === 0) {
			inRealityOpts = true;
			realityIndent = indent;
			continue;
		}

		if (inRealityOpts) {
			if (!publicKey && key === "public-key") {
				publicKey = parseYamlScalar(value);
			}
			if (!shortId && key === "short-id") {
				shortId = parseYamlScalar(value);
			}
		}
	}

	return { servername, publicKey, shortId };
}

type TokenPart = { text: string; kind: string; start: number };

type Range = { start: number; end: number };

function tokenizeJsonLine(line: string): TokenPart[] {
	const parts: TokenPart[] = [];

	let i = 0;
	while (i < line.length) {
		const ch = line[i];

		if (ch === '"') {
			let j = i + 1;
			let escaped = false;
			while (j < line.length) {
				const c = line[j];
				if (escaped) {
					escaped = false;
					j += 1;
					continue;
				}
				if (c === "\\") {
					escaped = true;
					j += 1;
					continue;
				}
				if (c === '"') {
					j += 1;
					break;
				}
				j += 1;
			}
			parts.push({ text: line.slice(i, j), kind: "string", start: i });
			i = j;
			continue;
		}

		if ((ch >= "0" && ch <= "9") || ch === "-") {
			let j = i + 1;
			while (j < line.length) {
				const c = line[j];
				const isNumChar =
					(c >= "0" && c <= "9") ||
					c === "." ||
					c === "e" ||
					c === "E" ||
					c === "+" ||
					c === "-";
				if (!isNumChar) break;
				j += 1;
			}
			parts.push({ text: line.slice(i, j), kind: "number", start: i });
			i = j;
			continue;
		}

		const rest = line.slice(i);
		if (
			rest.startsWith("true") ||
			rest.startsWith("false") ||
			rest.startsWith("null")
		) {
			const kw = rest.startsWith("true")
				? "true"
				: rest.startsWith("false")
					? "false"
					: "null";
			const next = line[i + kw.length] ?? "";
			if (!next || /[^A-Za-z0-9_]/.test(next)) {
				parts.push({ text: kw, kind: "keyword", start: i });
				i += kw.length;
				continue;
			}
		}

		if ("{}[],:".includes(ch)) {
			parts.push({ text: ch, kind: "punct", start: i });
			i += 1;
			continue;
		}

		parts.push({ text: ch, kind: "plain", start: i });
		i += 1;
	}

	return parts;
}

function tokenizeYamlLine(line: string): TokenPart[] {
	const parts: TokenPart[] = [];
	let pos = 0;
	const push = (text: string, kind: string) => {
		parts.push({ text, kind, start: pos });
		pos += text.length;
	};

	const trimmed = line.trim();
	if (!trimmed) return [{ text: line, kind: "plain", start: 0 }];
	if (trimmed.startsWith("#"))
		return [{ text: line, kind: "comment", start: 0 }];

	const indentLen = line.length - line.trimStart().length;
	const indent = line.slice(0, indentLen);
	let rest = line.slice(indentLen);

	let listPrefix = "";
	if (rest.startsWith("- ")) {
		listPrefix = "- ";
		rest = rest.slice(2);
	}

	const keyMatch = rest.match(/^([A-Za-z0-9_-]+)(\s*:\s*)(.*)$/);
	if (!keyMatch) return [{ text: line, kind: "plain", start: 0 }];

	const key = keyMatch[1] ?? "";
	const sep = keyMatch[2] ?? ": ";
	const valueAndMaybeComment = keyMatch[3] ?? "";

	let value = valueAndMaybeComment;
	let comment = "";
	const hashIdx = valueAndMaybeComment.indexOf(" #");
	if (hashIdx >= 0) {
		value = valueAndMaybeComment.slice(0, hashIdx);
		comment = valueAndMaybeComment.slice(hashIdx);
	}

	push(indent, "plain");
	if (listPrefix) push(listPrefix, "punct");
	push(key, "key");
	push(sep, "plain");

	const scalar = value.trim();
	if (scalar.length === 0) {
		push(value, "plain");
	} else if (
		(scalar.startsWith('"') && scalar.endsWith('"')) ||
		(scalar.startsWith("'") && scalar.endsWith("'"))
	) {
		const leading = value.slice(0, value.indexOf(scalar));
		const trailing = value.slice(leading.length + scalar.length);
		if (leading) push(leading, "plain");
		push(scalar, "string");
		if (trailing) push(trailing, "plain");
	} else if (/^-?\d+(\.\d+)?([eE][+-]?\d+)?$/.test(scalar)) {
		const leading = value.slice(0, value.indexOf(scalar));
		const trailing = value.slice(leading.length + scalar.length);
		if (leading) push(leading, "plain");
		push(scalar, "number");
		if (trailing) push(trailing, "plain");
	} else if (scalar === "true" || scalar === "false" || scalar === "null") {
		const leading = value.slice(0, value.indexOf(scalar));
		const trailing = value.slice(leading.length + scalar.length);
		if (leading) push(leading, "plain");
		push(scalar, "keyword");
		if (trailing) push(trailing, "plain");
	} else {
		push(value, "plain");
	}

	if (comment) push(comment, "comment");
	return parts;
}

function TokenSpan({
	kind,
	text,
	matchRanges,
	tokenStart,
}: {
	kind: string;
	text: string;
	matchRanges: Range[] | null;
	tokenStart: number;
}) {
	const colorByKind: Record<string, string> = {
		plain: "var(--xp-code-plain)",
		punct: "var(--xp-code-punct)",
		key: "var(--xp-code-key)",
		string: "var(--xp-code-string)",
		number: "var(--xp-code-number)",
		keyword: "var(--xp-code-keyword)",
		comment: "var(--xp-code-comment)",
	};

	const color = colorByKind[kind] ?? colorByKind.plain;
	const matchBg = "var(--xp-code-highlight)";

	if (!matchRanges || matchRanges.length === 0) {
		return <span style={{ color }}>{text}</span>;
	}

	const tokenEnd = tokenStart + text.length;
	const segments: Array<{ text: string; highlighted: boolean }> = [];

	let cursor = 0;
	for (const r of matchRanges) {
		if (r.end <= tokenStart || r.start >= tokenEnd) continue;
		const startInToken = Math.max(r.start, tokenStart) - tokenStart;
		const endInToken = Math.min(r.end, tokenEnd) - tokenStart;
		if (startInToken > cursor) {
			segments.push({
				text: text.slice(cursor, startInToken),
				highlighted: false,
			});
		}
		segments.push({
			text: text.slice(startInToken, endInToken),
			highlighted: true,
		});
		cursor = endInToken;
	}
	if (cursor < text.length) {
		segments.push({ text: text.slice(cursor), highlighted: false });
	}

	return (
		<>
			{segments.map((s, idx) => (
				<span
					key={`${tokenStart}:${String(idx)}`}
					style={{
						color,
						backgroundColor: s.highlighted ? matchBg : "transparent",
						borderRadius: s.highlighted ? 6 : 0,
						boxDecorationBreak: "clone",
					}}
				>
					{s.text}
				</span>
			))}
		</>
	);
}

async function writeClipboard(text: string): Promise<void> {
	try {
		await navigator.clipboard.writeText(text);
	} catch {}
}

function CodeView({
	text,
	language,
	activeLine,
	highlight,
	fillHeight = false,
}: {
	text: string;
	language: CodeLanguage;
	activeLine: number | null;
	highlight: string;
	fillHeight?: boolean;
}) {
	const lines = useMemo(() => text.split("\n"), [text]);
	const lineEntries = useMemo(
		() => lines.map((line, idx) => ({ line, lineIdx: idx, lineNo: idx + 1 })),
		[lines],
	);

	const codeScrollRef = useRef<HTMLDivElement | null>(null);
	const gutterInnerRef = useRef<HTMLDivElement | null>(null);
	const hTrackRef = useRef<HTMLDivElement | null>(null);
	const hThumbRef = useRef<HTMLDivElement | null>(null);
	const vTrackRef = useRef<HTMLDivElement | null>(null);
	const vThumbRef = useRef<HTMLDivElement | null>(null);
	const scheduleScrollbarUpdateRef = useRef<(() => void) | null>(null);
	const [scrollbarVisibility, setScrollbarVisibility] = useState<{
		h: boolean;
		v: boolean;
	}>({ h: false, v: false });

	const highlightNeedle = useMemo(
		() => highlight.trim().toLowerCase(),
		[highlight],
	);
	const matchRangesByLine = useMemo(() => {
		if (!highlightNeedle) return new Map<number, Range[]>();
		const out = new Map<number, Range[]>();
		for (const e of lineEntries) {
			const hay = (e.line ?? "").toLowerCase();
			let at = hay.indexOf(highlightNeedle);
			if (at < 0) continue;
			const ranges: Range[] = [];
			while (at >= 0) {
				ranges.push({ start: at, end: at + highlightNeedle.length });
				at = hay.indexOf(highlightNeedle, at + highlightNeedle.length);
			}
			out.set(e.lineIdx, ranges);
		}
		return out;
	}, [highlightNeedle, lineEntries]);

	useEffect(() => {
		const el = codeScrollRef.current;
		const gutterInner = gutterInnerRef.current;
		if (!el || !gutterInner) return;

		let raf = 0;
		const onScroll = () => {
			cancelAnimationFrame(raf);
			raf = requestAnimationFrame(() => {
				gutterInner.style.transform = `translateY(-${el.scrollTop}px)`;
			});
		};

		onScroll();
		el.addEventListener("scroll", onScroll, { passive: true });
		return () => {
			cancelAnimationFrame(raf);
			el.removeEventListener("scroll", onScroll);
		};
	}, []);

	useEffect(() => {
		if (activeLine == null) return;
		const scroller = codeScrollRef.current;
		if (!scroller) return;
		const target = scroller.querySelector(
			`[data-line="${String(activeLine)}"]`,
		) as HTMLElement | null;
		if (!target) return;
		target.scrollIntoView({ block: "center" });
	}, [activeLine]);

	const codeStroke = "var(--xp-code-border)";
	const codeBg = "var(--xp-code-bg)";
	const activeLineBg = "var(--xp-code-active-line)";
	const codeDimColor = "var(--xp-code-comment)";
	const trackBg = "var(--xp-code-track)";
	const thumbBg = "var(--xp-code-thumb)";

	useEffect(() => {
		const scroller = codeScrollRef.current;
		const hTrack = hTrackRef.current;
		const hThumb = hThumbRef.current;
		const vTrack = vTrackRef.current;
		const vThumb = vThumbRef.current;
		if (!scroller || !hTrack || !hThumb || !vTrack || !vThumb) return;

		let raf = 0;
		const minThumb = 28;

		const update = () => {
			const hVisible = scroller.scrollWidth > scroller.clientWidth + 1;
			const vVisible = scroller.scrollHeight > scroller.clientHeight + 1;

			const hTrackWidth = hTrack.clientWidth;
			const vTrackHeight = vTrack.clientHeight;

			if (hVisible && hTrackWidth > 0) {
				const ratio = scroller.clientWidth / scroller.scrollWidth;
				const thumbWidth = Math.max(minThumb, Math.round(hTrackWidth * ratio));
				const maxLeft = Math.max(0, hTrackWidth - thumbWidth);
				const denom = Math.max(1, scroller.scrollWidth - scroller.clientWidth);
				const left = Math.round((scroller.scrollLeft / denom) * maxLeft);
				hThumb.style.width = `${thumbWidth}px`;
				hThumb.style.transform = `translateX(${left}px)`;
			}

			if (vVisible && vTrackHeight > 0) {
				const ratio = scroller.clientHeight / scroller.scrollHeight;
				const thumbHeight = Math.max(
					minThumb,
					Math.round(vTrackHeight * ratio),
				);
				const maxTop = Math.max(0, vTrackHeight - thumbHeight);
				const denom = Math.max(
					1,
					scroller.scrollHeight - scroller.clientHeight,
				);
				const top = Math.round((scroller.scrollTop / denom) * maxTop);
				vThumb.style.height = `${thumbHeight}px`;
				vThumb.style.transform = `translateY(${top}px)`;
			}

			setScrollbarVisibility((prev) => {
				if (prev.h === hVisible && prev.v === vVisible) return prev;
				return { h: hVisible, v: vVisible };
			});
		};

		const scheduleUpdate = () => {
			cancelAnimationFrame(raf);
			raf = requestAnimationFrame(update);
		};

		scheduleScrollbarUpdateRef.current = scheduleUpdate;
		scheduleUpdate();
		scroller.addEventListener("scroll", scheduleUpdate, { passive: true });

		if (typeof ResizeObserver !== "undefined") {
			const ro = new ResizeObserver(scheduleUpdate);
			ro.observe(scroller);
			ro.observe(hTrack);
			ro.observe(vTrack);
			return () => {
				cancelAnimationFrame(raf);
				scroller.removeEventListener("scroll", scheduleUpdate);
				ro.disconnect();
				scheduleScrollbarUpdateRef.current = null;
			};
		}

		const onResize = () => scheduleUpdate();
		window.addEventListener("resize", onResize);
		return () => {
			cancelAnimationFrame(raf);
			scroller.removeEventListener("scroll", scheduleUpdate);
			window.removeEventListener("resize", onResize);
			scheduleScrollbarUpdateRef.current = null;
		};
	}, []);

	useEffect(() => {
		scheduleScrollbarUpdateRef.current?.();
	});

	const beginDrag = (e: ReactPointerEvent<HTMLDivElement>, axis: "x" | "y") => {
		const scroller = codeScrollRef.current;
		const hTrack = hTrackRef.current;
		const vTrack = vTrackRef.current;
		const hThumb = hThumbRef.current;
		const vThumb = vThumbRef.current;
		if (!scroller || !hTrack || !vTrack || !hThumb || !vThumb) return;

		e.preventDefault();

		const startX = e.clientX;
		const startY = e.clientY;
		const startScrollLeft = scroller.scrollLeft;
		const startScrollTop = scroller.scrollTop;

		const hTrackWidth = hTrack.clientWidth;
		const vTrackHeight = vTrack.clientHeight;
		const hThumbWidth = hThumb.getBoundingClientRect().width;
		const vThumbHeight = vThumb.getBoundingClientRect().height;

		const maxHThumbLeft = Math.max(1, hTrackWidth - hThumbWidth);
		const maxVThumbTop = Math.max(1, vTrackHeight - vThumbHeight);
		const maxScrollLeft = Math.max(
			1,
			scroller.scrollWidth - scroller.clientWidth,
		);
		const maxScrollTop = Math.max(
			1,
			scroller.scrollHeight - scroller.clientHeight,
		);

		const onMove = (ev: PointerEvent) => {
			if (axis === "x") {
				const dx = ev.clientX - startX;
				const next = startScrollLeft + (dx / maxHThumbLeft) * maxScrollLeft;
				scroller.scrollLeft = Math.max(0, Math.min(maxScrollLeft, next));
				return;
			}

			const dy = ev.clientY - startY;
			const next = startScrollTop + (dy / maxVThumbTop) * maxScrollTop;
			scroller.scrollTop = Math.max(0, Math.min(maxScrollTop, next));
		};

		const onUp = () => {
			window.removeEventListener("pointermove", onMove);
			window.removeEventListener("pointerup", onUp);
		};

		window.addEventListener("pointermove", onMove);
		window.addEventListener("pointerup", onUp, { once: true });
	};

	return (
		<div
			className={[
				"rounded-[14px] border overflow-hidden relative",
				fillHeight
					? "h-[min(52vh,508px)] min-h-[260px] sm:min-h-[320px] xl:h-[508px]"
					: "h-[min(56vh,520px)] min-h-[260px] sm:min-h-[320px]",
			].join(" ")}
			style={{ borderColor: codeStroke, backgroundColor: codeBg }}
		>
			<div className="flex h-full">
				<div
					className="shrink-0 overflow-hidden font-mono tabular-nums select-none"
					style={{
						width: "56px",
						minWidth: "56px",
						backgroundColor: "var(--xp-code-gutter)",
					}}
					aria-hidden
				>
					<div ref={gutterInnerRef} className="pt-[30px] pb-6">
						{lineEntries.map((e) => (
							<div
								key={e.lineNo}
								className={[
									"h-[22px] text-center text-[11px] leading-[22px]",
									activeLine === e.lineIdx ? "" : "",
								]
									.filter(Boolean)
									.join(" ")}
								style={{
									color: codeDimColor,
									backgroundColor:
										activeLine === e.lineIdx ? activeLineBg : "transparent",
								}}
							>
								{String(e.lineNo).padStart(2, "0")}
							</div>
						))}
					</div>
				</div>
				<div
					ref={codeScrollRef}
					className="flex-1 overflow-auto font-mono font-normal text-[12px] h-full [&::-webkit-scrollbar]:hidden"
					data-testid="subscription-code-scroll"
					style={{
						scrollbarWidth: "none",
						msOverflowStyle: "none",
					}}
				>
					<div className="min-w-max pt-[30px] pb-6">
						{lineEntries.map((e) => {
							const isActive = activeLine === e.lineIdx;
							const matchRanges = matchRangesByLine.get(e.lineIdx) ?? null;
							const tokens =
								language === "yaml"
									? tokenizeYamlLine(e.line)
									: language === "json"
										? tokenizeJsonLine(e.line)
										: [{ text: e.line, kind: "plain", start: 0 }];
							return (
								<div
									key={e.lineNo}
									data-line={e.lineIdx}
									className={[
										"pl-[18px] pr-3 leading-[22px] whitespace-pre",
										isActive ? "" : "",
									]
										.filter(Boolean)
										.join(" ")}
									style={{
										backgroundColor: isActive ? activeLineBg : "transparent",
									}}
								>
									{tokens.map((t) => (
										<TokenSpan
											key={`${t.start}:${t.kind}`}
											kind={t.kind}
											text={t.text}
											matchRanges={matchRanges}
											tokenStart={t.start}
										/>
									))}
								</div>
							);
						})}
					</div>
				</div>
			</div>

			{/* Custom scrollbars: styled like design, but reflect real scroll state. */}
			<div
				ref={hTrackRef}
				className={[
					"absolute left-[68px] right-[12px] bottom-[4px] h-[6px] rounded-[3px]",
					scrollbarVisibility.h ? "" : "opacity-0",
				].join(" ")}
				aria-hidden
				style={{ backgroundColor: trackBg }}
			>
				<div
					ref={hThumbRef}
					className="absolute top-0 left-0 h-[6px] rounded-[3px] cursor-grab active:cursor-grabbing"
					style={{ backgroundColor: thumbBg }}
					onPointerDown={(e) => beginDrag(e, "x")}
				/>
			</div>
			<div
				ref={vTrackRef}
				className={[
					"absolute right-[6px] top-[18px] bottom-[14px] w-[6px] rounded-[3px]",
					scrollbarVisibility.v ? "" : "opacity-0",
				].join(" ")}
				aria-hidden
				style={{ backgroundColor: trackBg }}
			>
				<div
					ref={vThumbRef}
					className="absolute left-0 top-0 w-[6px] rounded-[3px] cursor-grab active:cursor-grabbing"
					style={{ backgroundColor: thumbBg }}
					onPointerDown={(e) => beginDrag(e, "y")}
				/>
			</div>
		</div>
	);
}

export function SubscriptionPreviewDialog({
	open,
	onClose,
	subscriptionUrl,
	format,
	loading,
	content,
	error,
}: SubscriptionPreviewDialogProps) {
	const searchInputId = useId();

	const language = useMemo(
		() => chooseLanguage(format, content),
		[content, format],
	);
	const fields = useMemo(
		() => (format === "clash" ? extractClashFields(content) : {}),
		[content, format],
	);
	const copyAllFieldsText = useMemo(() => {
		if (format !== "clash") return "";
		const parts: string[] = [];
		if (fields.publicKey) parts.push(`public-key: ${fields.publicKey}`);
		if (fields.shortId) parts.push(`short-id: ${fields.shortId}`);
		if (fields.servername) parts.push(`servername: ${fields.servername}`);
		return parts.join("\n");
	}, [fields.publicKey, fields.servername, fields.shortId, format]);

	const [searchQuery, setSearchQuery] = useState("");
	const [matchIndex, setMatchIndex] = useState(0);

	const matches = useMemo(() => {
		const q = searchQuery.trim();
		if (!q) return [];
		const needle = q.toLowerCase();
		const lines = content.split("\n");
		const out: Array<{ line: number; at: number }> = [];
		for (let lineIdx = 0; lineIdx < lines.length; lineIdx += 1) {
			const hay = (lines[lineIdx] ?? "").toLowerCase();
			let at = hay.indexOf(needle);
			while (at >= 0) {
				out.push({ line: lineIdx, at });
				at = hay.indexOf(needle, at + needle.length);
			}
		}
		return out;
	}, [content, searchQuery]);

	const activeLine =
		matches.length > 0 ? (matches[matchIndex]?.line ?? 0) : null;
	const showFieldsPanel = format === "clash";

	const headerBtnBase =
		"min-h-11 sm:min-h-10 rounded-xl border border-border bg-muted px-3 text-[12px] font-[750] text-foreground shadow-xs transition-colors hover:bg-accent focus-visible:outline-none focus-visible:ring-[3px] focus-visible:ring-ring/20";
	const searchBtnBase =
		"min-h-11 sm:min-h-10 rounded-xl border border-border bg-muted px-4 text-[12px] font-[750] text-foreground shadow-xs transition-colors hover:bg-accent focus-visible:outline-none focus-visible:ring-[3px] focus-visible:ring-ring/20";
	const contentPadClass = "px-4 sm:px-6 lg:pl-9 lg:pr-6";
	const closePadClass = "pr-3 sm:pr-6";

	const mutedTextClass = "text-muted-foreground";
	const mutedPlaceholderClass = "placeholder:text-muted-foreground";
	const fieldCopyButtonClass =
		"min-h-11 w-full rounded-xl border border-primary/25 bg-primary/10 px-3 text-[12px] font-[750] text-foreground shadow-xs transition-colors hover:bg-primary/15 focus-visible:outline-none focus-visible:ring-[3px] focus-visible:ring-ring/20 sm:w-auto sm:min-w-[5.5rem]";

	return (
		<Dialog open={open} onOpenChange={(next) => !next && onClose()}>
			<DialogContent
				showCloseButton={false}
				className={cn(
					"w-[calc(100vw-1rem)] max-w-[1160px] max-h-[calc(100dvh-1rem)] overflow-x-hidden overflow-y-auto rounded-[18px] border border-border bg-card p-0 text-card-foreground shadow-sm sm:w-[calc(100vw-2rem)] sm:max-h-[calc(100dvh-2rem)] xl:overflow-hidden",
					showFieldsPanel ? "xl:h-[660px]" : "",
				)}
				data-sub-preview-dialog
			>
				<DialogTitle className="sr-only">
					Subscription content dialog
				</DialogTitle>
				<DialogDescription className="sr-only">
					Inspect the generated subscription content and copy derived connection
					fields.
				</DialogDescription>
				{/* Keep the close action pinned to the top-right for all sizes. */}
				<div className="sticky top-0 z-30 h-0 pointer-events-none">
					<div
						className={["flex justify-end pt-[13px]", closePadClass].join(" ")}
					>
						<button
							type="button"
							className="pointer-events-auto flex size-11 items-center justify-center rounded-full border border-border bg-background text-foreground shadow-xs transition-colors hover:bg-muted focus-visible:outline-none focus-visible:ring-[3px] focus-visible:ring-ring/20"
							aria-label="Close"
							data-sub-preview-close
							onClick={() => {
								onClose();
							}}
						>
							<Icon name="tabler:x" size={20} ariaLabel="Close" />
						</button>
					</div>
				</div>

				<div className={[contentPadClass, "pt-[13px] pb-[18px]"].join(" ")}>
					<div className="grid gap-3 pr-14 lg:grid-cols-[minmax(0,1fr)_auto]">
						<div className="flex min-h-11 min-w-0 flex-wrap items-center gap-3">
							<h3 className="text-xl font-[750] leading-7 text-foreground sm:text-[22px]">
								Subscription preview
							</h3>
							<div className="inline-flex items-center gap-4 min-w-0">
								<span className="inline-flex h-7 min-w-20 items-center justify-center rounded-[10px] border border-info/25 bg-info/10 px-3 text-[12px] font-[750] text-info">
									{format}
								</span>
								{loading ? (
									<span className="xp-loading-spinner xp-loading-spinner-xs" />
								) : null}
							</div>
						</div>

						<div className="flex min-h-11 flex-wrap items-center gap-2 lg:justify-end">
							<button
								type="button"
								className={headerBtnBase}
								onClick={async () => {
									await writeClipboard(subscriptionUrl);
								}}
							>
								Copy URL
							</button>
							<button
								type="button"
								className={headerBtnBase}
								onClick={async () => {
									await writeClipboard(content);
								}}
							>
								Copy content
							</button>
						</div>

						<div className="grid min-w-0 gap-2 sm:grid-cols-[auto_minmax(0,1fr)_auto] sm:items-center lg:col-span-2">
							<label
								htmlFor={searchInputId}
								className={cn("text-[12px] leading-none", mutedTextClass)}
							>
								Search
							</label>
							<Input
								id={searchInputId}
								className={cn(
									"h-10 min-w-0 rounded-xl border border-input bg-background px-4 font-mono text-[12px] text-foreground shadow-xs outline-none",
									mutedPlaceholderClass,
								)}
								value={searchQuery}
								onChange={(e) => {
									setSearchQuery(e.target.value);
									setMatchIndex(0);
								}}
								placeholder="e.g. public-key / short-id / servername"
							/>
							<div className="flex min-w-0 flex-wrap items-center gap-2 sm:justify-end">
								<button
									type="button"
									className={searchBtnBase}
									onClick={() => {
										if (matches.length === 0) return;
										setMatchIndex((prev) => (prev + 1) % matches.length);
									}}
								>
									Find next
								</button>
								<button
									type="button"
									className={searchBtnBase}
									onClick={() => {
										if (matches.length === 0) return;
										setMatchIndex(
											(prev) => (prev - 1 + matches.length) % matches.length,
										);
									}}
								>
									Find prev
								</button>
							</div>
						</div>
					</div>
				</div>

				<div className={[contentPadClass, "pb-[28px]"].join(" ")}>
					{error ? (
						<div className="text-sm text-destructive">{error}</div>
					) : null}

					<div
						className={[
							"grid gap-4",
							showFieldsPanel
								? "xl:grid-cols-[minmax(0,1fr)_264px]"
								: "lg:grid-cols-1",
						].join(" ")}
					>
						<div className="min-w-0">
							<CodeView
								text={content}
								language={language}
								activeLine={activeLine}
								highlight={searchQuery}
								fillHeight={showFieldsPanel}
							/>
						</div>

						{showFieldsPanel ? (
							<div className="space-y-3 overflow-hidden rounded-[14px] border border-border bg-muted/35 p-4 xl:h-[508px]">
								<div className="space-y-1">
									<h4 className="text-[13px] font-[750] text-foreground">
										Fields
									</h4>
									<div className={["text-[12px]", mutedTextClass].join(" ")}>
										Click Copy to copy exact value
									</div>
								</div>

								<div className="space-y-4">
									<div className="space-y-1">
										<div
											className={["font-mono text-[12px]", mutedTextClass].join(
												" ",
											)}
										>
											public-key
										</div>
										<div className="grid items-center gap-2 sm:grid-cols-[minmax(0,1fr)_minmax(5.5rem,auto)] xl:grid-cols-[154px_minmax(5.5rem,auto)]">
											<div
												className="flex h-11 items-center overflow-hidden rounded-xl border border-input bg-background px-4 font-mono text-[13px] text-foreground"
												title={fields.publicKey ?? ""}
											>
												<div className="whitespace-nowrap">
													{fields.publicKey
														? truncateMiddle(fields.publicKey, 4, 5)
														: "—"}
												</div>
											</div>
											{fields.publicKey ? (
												<button
													type="button"
													className={fieldCopyButtonClass}
													aria-label="Copy public-key"
													onClick={async () => {
														await writeClipboard(fields.publicKey ?? "");
													}}
												>
													Copy
												</button>
											) : null}
										</div>
									</div>

									<div className="space-y-1">
										<div
											className={["font-mono text-[12px]", mutedTextClass].join(
												" ",
											)}
										>
											short-id
										</div>
										<div className="grid items-center gap-2 sm:grid-cols-[minmax(0,1fr)_minmax(5.5rem,auto)] xl:grid-cols-[154px_minmax(5.5rem,auto)]">
											<div
												className="flex h-11 items-center overflow-hidden rounded-xl border border-input bg-background px-4 font-mono text-[13px] text-foreground"
												title={fields.shortId ?? ""}
											>
												<div className="whitespace-nowrap">
													{fields.shortId
														? truncateMiddle(fields.shortId, 6, 4)
														: "—"}
												</div>
											</div>
											{fields.shortId ? (
												<button
													type="button"
													className={fieldCopyButtonClass}
													aria-label="Copy short-id"
													onClick={async () => {
														await writeClipboard(fields.shortId ?? "");
													}}
												>
													Copy
												</button>
											) : null}
										</div>
									</div>

									<div className="space-y-1">
										<div
											className={["font-mono text-[12px]", mutedTextClass].join(
												" ",
											)}
										>
											servername
										</div>
										<div className="grid items-center gap-2 sm:grid-cols-[minmax(0,1fr)_minmax(5.5rem,auto)] xl:grid-cols-[154px_minmax(5.5rem,auto)]">
											<div
												className="flex h-11 items-center overflow-hidden rounded-xl border border-input bg-background px-4 font-mono text-[13px] text-foreground"
												title={fields.servername ?? ""}
											>
												<div className="whitespace-nowrap">
													{fields.servername ?? "—"}
												</div>
											</div>
											{fields.servername ? (
												<button
													type="button"
													className={fieldCopyButtonClass}
													aria-label="Copy servername"
													onClick={async () => {
														await writeClipboard(fields.servername ?? "");
													}}
												>
													Copy
												</button>
											) : null}
										</div>
									</div>

									{copyAllFieldsText ? (
										<button
											type="button"
											className="min-h-11 w-full rounded-xl border border-primary/25 bg-primary/10 px-4 text-[12px] font-[750] text-foreground shadow-xs transition-colors hover:bg-primary/15 focus-visible:outline-none focus-visible:ring-[3px] focus-visible:ring-ring/20"
											onClick={async () => {
												await writeClipboard(copyAllFieldsText);
											}}
										>
											Copy all fields
										</button>
									) : null}

									<div className={["text-[12px]", mutedTextClass].join(" ")}>
										Tip: horizontal scroll for long lines
									</div>
								</div>
							</div>
						) : null}
					</div>
				</div>
			</DialogContent>
		</Dialog>
	);
}

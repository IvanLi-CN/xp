import type { PointerEvent as ReactPointerEvent } from "react";
import { useEffect, useMemo, useRef, useState } from "react";

import type { SubscriptionFormat } from "../api/subscription";
import { useUiPrefsOptional } from "./UiPrefs";

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
	if (format === "clash") return "yaml";
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
	theme,
	matchRanges,
	tokenStart,
}: {
	kind: string;
	text: string;
	theme: "light" | "dark";
	matchRanges: Range[] | null;
	tokenStart: number;
}) {
	const colorByKind: Record<string, string> = {
		plain: "#e2e8f0",
		punct: "#cbd5e1",
		key: "#93c5fd",
		string: "#a7f3d0",
		number: "#fcd34d",
		keyword: "#fda4af",
		comment: "#94a3b8",
	};

	const color = colorByKind[kind] ?? colorByKind.plain;
	const matchBg =
		theme === "light" ? "rgba(168,85,247,0.20)" : "rgba(168,85,247,0.28)";

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
	theme,
	highlight,
	fillHeight = false,
}: {
	text: string;
	language: CodeLanguage;
	activeLine: number | null;
	theme: "light" | "dark";
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

	const codeStroke = theme === "light" ? "#1f2a44" : "#22304a";
	const codeBg = theme === "light" ? "#0b1220" : "#050817";
	const gutterBg = theme === "light" ? "bg-[#0f172a]/75" : "bg-[#0f172a]/90";
	const activeLineBg =
		theme === "light" ? "rgba(168,85,247,0.12)" : "rgba(168,85,247,0.16)";
	const codeDimColor = "#94a3b8";

	const trackBg =
		theme === "light" ? "rgba(17,28,51,0.9)" : "rgba(17,28,51,0.95)";
	const thumbBg = "rgba(34,211,238,0.85)";

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
				fillHeight ? "max-h-[40vh] lg:h-[508px] lg:max-h-none" : "max-h-[56vh]",
			].join(" ")}
			style={{ borderColor: codeStroke, backgroundColor: codeBg }}
		>
			<div className="flex h-full">
				<div
					className={[
						"shrink-0 overflow-hidden font-mono tabular-nums select-none",
						gutterBg,
					].join(" ")}
					style={{
						width: "56px",
						minWidth: "56px",
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
											theme={theme}
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
	const prefs = useUiPrefsOptional();
	const resolvedTheme =
		prefs?.resolvedTheme ??
		(typeof document !== "undefined" &&
		document.documentElement.getAttribute("data-theme") === "xp-light"
			? "light"
			: "dark");

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

	const theme =
		resolvedTheme === "light"
			? {
					modalBg: "bg-white border-[#e2e8f0] text-[#0f172a]",
					stroke: "#e2e8f0",
					btnBg: "#f1f5f9",
					btnText: "#0f172a",
					inputBg: "#ffffff",
					inputText: "#0f172a",
					muted: "#64748b",
					fieldsBg: "#f8fafc",
					fieldsTitle: "#0f172a",
					fieldsCopyBg: "#f1f5f9",
					fieldsCopyText: "#0f172a",
					copyAllBg: "#e6fbff",
					copyAllText: "#062a30",
					codeBg: "#050817",
				}
			: {
					modalBg: "bg-[#0f172a] border-[#24324a] text-slate-200",
					stroke: "#24324a",
					btnBg: "#111c33",
					btnText: "#e2e8f0",
					inputBg: "#0f172a",
					inputText: "#e2e8f0",
					muted: "#94a3b8",
					fieldsBg: "#111c33",
					fieldsTitle: "#e2e8f0",
					fieldsCopyBg: "#22d3ee",
					fieldsCopyText: "#031c22",
					copyAllBg: "#22d3ee2e",
					copyAllText: "#e2e8f0",
					codeBg: "#050817",
				};

	const headerBtnBase =
		"h-[34px] w-[120px] rounded-[10px] border text-[12px] font-[750] !shadow-none";
	const headerBtnWide = "w-[140px]";
	const searchBtnBase =
		"h-9 px-6 rounded-xl border text-[12px] font-[750] whitespace-nowrap !shadow-none";
	const contentPadClass = "pl-[36px] pr-[24px]";
	const closePadClass = "pr-[24px]";
	// Match close button size (44px) + intended gap (12px).
	const closeLaneSpacerClass = "w-[56px]";

	const mutedTextClass =
		resolvedTheme === "light" ? "text-[#64748b]" : "text-[#94a3b8]";
	const mutedPlaceholderClass =
		resolvedTheme === "light"
			? "placeholder:text-[#64748b]"
			: "placeholder:text-[#94a3b8]";

	return (
		<dialog
			className="modal"
			open={open}
			onCancel={(e) => {
				e.preventDefault();
				onClose();
			}}
			style={{
				backgroundColor:
					resolvedTheme === "light"
						? "rgba(15,23,42,0.45)"
						: "rgba(0,0,0,0.38)",
				backdropFilter: "none",
				WebkitBackdropFilter: "none",
			}}
		>
			<div
				className={[
					"modal-box w-[calc(100vw-64px)] max-w-[1160px] p-0 border rounded-[18px] !shadow-none overflow-x-hidden overflow-y-auto lg:overflow-hidden max-h-[calc(100vh-64px)]",
					showFieldsPanel ? "lg:h-[660px]" : "",
					theme.modalBg,
				].join(" ")}
				data-sub-preview-dialog
			>
				{/* Keep the close action pinned to the top-right for all sizes. */}
				<div className="sticky top-0 z-30 h-0 pointer-events-none">
					<div
						className={["flex justify-end pt-[13px]", closePadClass].join(" ")}
					>
						<button
							type="button"
							className="w-11 h-11 rounded-full border !shadow-none flex items-center justify-center pointer-events-auto"
							style={{
								borderColor: theme.stroke,
								backgroundColor: theme.inputBg,
								color: theme.btnText,
							}}
							aria-label="Close"
							data-sub-preview-close
							onClick={() => {
								onClose();
							}}
						>
							<span className="text-[20px] leading-none font-[900]">×</span>
						</button>
					</div>
				</div>

				<div className={[contentPadClass, "pt-[13px] pb-[18px]"].join(" ")}>
					<div className="grid gap-0 md:gap-x-3 md:grid-cols-[minmax(0,1fr)_minmax(0,372px)]">
						<div className="h-11 flex items-center gap-3 min-w-0">
							<h3
								className="text-[22px] leading-[28px] font-[750] whitespace-nowrap"
								style={{ color: theme.btnText }}
							>
								Subscription preview
							</h3>
							<div className="inline-flex items-center gap-4 min-w-0">
								<span
									className={[
										"inline-flex items-center justify-center w-[90px] h-7 rounded-[10px] border text-[12px] font-[750]",
										resolvedTheme === "light"
											? "bg-[#e6fbff] border-[#e2e8f0] text-[#062a30]"
											: "bg-[#22d3ee2e] border-[#24324a] text-slate-200",
									].join(" ")}
								>
									{format}
								</span>
								{loading ? (
									<span className="loading loading-spinner loading-xs" />
								) : null}
							</div>
						</div>

						<div className="min-h-11 flex items-center justify-end flex-wrap">
							<div className="flex items-center gap-2">
								<button
									type="button"
									className={headerBtnBase}
									style={{
										backgroundColor: theme.btnBg,
										borderColor: theme.stroke,
										color: theme.btnText,
									}}
									onClick={async () => {
										await writeClipboard(subscriptionUrl);
									}}
								>
									Copy URL
								</button>
								<button
									type="button"
									className={[headerBtnBase, headerBtnWide].join(" ")}
									style={{
										backgroundColor: theme.btnBg,
										borderColor: theme.stroke,
										color: theme.btnText,
									}}
									onClick={async () => {
										await writeClipboard(content);
									}}
								>
									Copy content
								</button>
							</div>
							{/* Reserve a lane for the pinned close button so actions never sit underneath it. */}
							<div className={closeLaneSpacerClass} aria-hidden />
						</div>

						<div className="mt-[13px] flex flex-wrap items-center gap-3 min-w-0 md:col-span-2">
							<span
								className={[
									"w-[52px] shrink-0 text-[12px] leading-none",
									mutedTextClass,
								].join(" ")}
							>
								Search
							</span>
							<input
								className={[
									"flex-1 min-w-[240px] h-9 rounded-xl border px-4 font-mono text-[12px] !shadow-none outline-none",
									mutedPlaceholderClass,
								].join(" ")}
								style={{
									backgroundColor: theme.inputBg,
									borderColor: theme.stroke,
									color: theme.inputText,
								}}
								value={searchQuery}
								onChange={(e) => {
									setSearchQuery(e.target.value);
									setMatchIndex(0);
								}}
								placeholder="e.g. public-key / short-id / servername"
							/>
							<div className="ml-auto shrink-0 flex items-center gap-2">
								<button
									type="button"
									className={searchBtnBase}
									style={{
										backgroundColor: theme.btnBg,
										borderColor: theme.stroke,
										color: theme.btnText,
									}}
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
									style={{
										backgroundColor: theme.btnBg,
										borderColor: theme.stroke,
										color: theme.btnText,
									}}
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
					{error ? <div className="text-sm text-error">{error}</div> : null}

					<div
						className={[
							"grid gap-4",
							showFieldsPanel
								? "lg:grid-cols-[minmax(0,1fr)_264px]"
								: "lg:grid-cols-1",
						].join(" ")}
					>
						<div className="min-w-0">
							<CodeView
								text={content}
								language={language}
								activeLine={activeLine}
								theme={resolvedTheme}
								highlight={searchQuery}
								fillHeight={showFieldsPanel}
							/>
						</div>

						{showFieldsPanel ? (
							<div
								className={[
									"rounded-[14px] border p-4 space-y-3 overflow-hidden lg:h-[508px]",
								].join(" ")}
								style={{
									backgroundColor: theme.fieldsBg,
									borderColor: theme.stroke,
								}}
							>
								<div className="space-y-1">
									<h4
										className="text-[13px] font-[750]"
										style={{ color: theme.fieldsTitle }}
									>
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
										<div className="grid grid-cols-[minmax(0,1fr)_70px] items-center gap-2 lg:grid-cols-[154px_70px]">
											<div
												className={[
													"h-11 rounded-xl border px-4 flex items-center overflow-hidden font-mono text-[13px] text-slate-200",
												].join(" ")}
												style={{
													backgroundColor: theme.inputBg,
													borderColor: theme.stroke,
												}}
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
													className="h-8 w-[70px] rounded-[10px] border text-[12px] font-[750]"
													style={{
														backgroundColor: theme.fieldsCopyBg,
														borderColor: theme.stroke,
														color: theme.fieldsCopyText,
													}}
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
										<div className="grid grid-cols-[minmax(0,1fr)_70px] items-center gap-2 lg:grid-cols-[154px_70px]">
											<div
												className={[
													"h-11 rounded-xl border px-4 flex items-center overflow-hidden font-mono text-[13px] text-slate-200",
												].join(" ")}
												style={{
													backgroundColor: theme.inputBg,
													borderColor: theme.stroke,
												}}
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
													className="h-8 w-[70px] rounded-[10px] border text-[12px] font-[750]"
													style={{
														backgroundColor: theme.fieldsCopyBg,
														borderColor: theme.stroke,
														color: theme.fieldsCopyText,
													}}
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
										<div className="grid grid-cols-[minmax(0,1fr)_70px] items-center gap-2 lg:grid-cols-[154px_70px]">
											<div
												className={[
													"h-11 rounded-xl border px-4 flex items-center overflow-hidden font-mono text-[13px] text-slate-200",
												].join(" ")}
												style={{
													backgroundColor: theme.inputBg,
													borderColor: theme.stroke,
												}}
												title={fields.servername ?? ""}
											>
												<div className="whitespace-nowrap">
													{fields.servername ?? "—"}
												</div>
											</div>
											{fields.servername ? (
												<button
													type="button"
													className="h-8 w-[70px] rounded-[10px] border text-[12px] font-[750]"
													style={{
														backgroundColor: theme.fieldsCopyBg,
														borderColor: theme.stroke,
														color: theme.fieldsCopyText,
													}}
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
											className="w-full h-10 rounded-xl border text-[12px] font-[750]"
											style={{
												backgroundColor: theme.copyAllBg,
												borderColor: theme.stroke,
												color: theme.copyAllText,
											}}
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
			</div>

			<form method="dialog" className="modal-backdrop">
				<button type="button" aria-label="close modal" onClick={onClose} />
			</form>
		</dialog>
	);
}

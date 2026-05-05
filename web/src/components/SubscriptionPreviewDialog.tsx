import { yaml } from "@codemirror/lang-yaml";
import { githubDark, githubLight } from "@uiw/codemirror-theme-github";
import CodeMirror from "@uiw/react-codemirror";
import { useMemo } from "react";

import { cn } from "@/lib/utils";
import type { SubscriptionFormat } from "../api/subscription";
import { EditorShortcutHint } from "./EditorShortcutHint";
import { Icon } from "./Icon";
import { SubscriptionFormatSegmentedControl } from "./SubscriptionFormatSegmentedControl";
import { useUiPrefsOptional } from "./UiPrefs";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogTitle,
} from "./ui/dialog";
import { Textarea } from "./ui/textarea";

type SubscriptionPreviewDialogProps = {
	open: boolean;
	onClose: () => void;
	subscriptionUrl: string;
	format: SubscriptionFormat;
	loading: boolean;
	content: string;
	error?: string | null;
	onFormatChange?: (format: SubscriptionFormat) => void | Promise<void>;
};

type ClashFields = {
	servername?: string;
	publicKey?: string;
	shortId?: string;
};

const CODEMIRROR_BASIC_SETUP = {
	lineNumbers: true,
	highlightActiveLineGutter: true,
	foldGutter: true,
	allowMultipleSelections: true,
	indentOnInput: true,
	bracketMatching: true,
	closeBrackets: true,
	autocompletion: true,
	highlightActiveLine: true,
	highlightSelectionMatches: true,
	searchKeymap: true,
	foldKeymap: true,
	completionKeymap: true,
	tabSize: 2,
};

const IS_TEST_MODE = import.meta.env.MODE === "test";

function truncateMiddle(value: string, head: number, tail: number): string {
	if (value.length <= head + tail + 1) return value;
	return `${value.slice(0, head)}…${value.slice(-tail)}`;
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

async function writeClipboard(text: string): Promise<void> {
	try {
		await navigator.clipboard.writeText(text);
	} catch {}
}

function SubscriptionContentEditor({
	content,
	format,
	fillHeight,
	loading,
}: {
	content: string;
	format: SubscriptionFormat;
	fillHeight: boolean;
	loading: boolean;
}) {
	const prefs = useUiPrefsOptional();
	const editorTheme =
		prefs?.resolvedTheme === "dark" ? githubDark : githubLight;
	const extensions = useMemo(
		() => (format === "clash" || format === "mihomo" ? [yaml()] : []),
		[format],
	);
	const height = fillHeight ? "508px" : "min(56vh, 520px)";
	const mobileHeightClass = fillHeight
		? "[&_.cm-editor]:max-lg:!h-[min(44dvh,420px)]"
		: "[&_.cm-editor]:max-lg:!h-[min(48dvh,420px)]";

	if (IS_TEST_MODE) {
		return (
			<div className="space-y-2">
				<div className="relative">
					<Textarea
						aria-label="Subscription content"
						className="h-[360px] resize-none font-mono text-sm"
						readOnly
						value={content}
						data-testid="subscription-code-scroll"
					/>
					{loading ? <EditorLoadingOverlay /> : null}
				</div>
				<EditorShortcutHint />
			</div>
		);
	}

	return (
		<div className="space-y-2">
			<div
				className={cn(
					"relative min-h-[260px] overflow-hidden rounded-[14px] border border-border bg-background",
					fillHeight ? "xl:h-[508px]" : "",
				)}
				data-testid="subscription-code-scroll"
			>
				<CodeMirror
					value={content}
					height={height}
					theme={editorTheme}
					extensions={extensions}
					basicSetup={CODEMIRROR_BASIC_SETUP}
					readOnly
					className={cn(
						"font-mono text-sm [&_.cm-editor]:min-h-[260px] [&_.cm-scroller]:overflow-auto",
						mobileHeightClass,
					)}
					aria-label="Subscription content"
				/>
				{loading ? <EditorLoadingOverlay /> : null}
			</div>
			<EditorShortcutHint />
		</div>
	);
}

function EditorLoadingOverlay() {
	return (
		<div className="absolute inset-0 z-10 flex items-center justify-center rounded-[14px] bg-background/70 backdrop-blur-[1px]">
			<div className="inline-flex items-center gap-2 rounded-full border border-border bg-card px-3 py-2 text-xs font-semibold text-foreground shadow-sm">
				<span className="xp-loading-spinner xp-loading-spinner-xs" />
				Loading content
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
	onFormatChange,
}: SubscriptionPreviewDialogProps) {
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
	const showFieldsPanel = format === "clash";

	const headerBtnBase =
		"inline-flex min-h-11 w-full items-center justify-center gap-2 rounded-xl border border-border bg-muted px-3 text-[12px] font-[750] text-foreground shadow-xs transition-colors hover:bg-accent focus-visible:outline-none focus-visible:ring-[3px] focus-visible:ring-ring/20 disabled:cursor-not-allowed disabled:opacity-60 sm:min-h-10 sm:w-auto";
	const headerIconBtnBase =
		"absolute right-4 top-4 flex size-10 items-center justify-center rounded-xl border border-border bg-muted text-foreground shadow-xs transition-colors hover:bg-accent focus-visible:outline-none focus-visible:ring-[3px] focus-visible:ring-ring/20 sm:right-5 sm:top-5";
	const contentPadClass = "px-4 sm:px-6 lg:pl-9 lg:pr-6";
	const mutedTextClass = "text-muted-foreground";
	const fieldValueClass =
		"block h-11 w-full min-w-0 overflow-hidden rounded-xl border border-input bg-background px-4 py-3 font-mono text-[13px] leading-5 text-foreground";
	const fieldCopyButtonClass =
		"min-h-11 w-full rounded-xl border border-primary/25 bg-primary/10 px-3 text-[12px] font-[750] text-foreground shadow-xs transition-colors hover:bg-primary/15 focus-visible:outline-none focus-visible:ring-[3px] focus-visible:ring-ring/20";

	return (
		<Dialog open={open} onOpenChange={(next) => !next && onClose()}>
			<DialogContent
				showCloseButton={false}
				className={cn(
					"w-[calc(100vw-1rem)] max-w-[1160px] max-h-[calc(100dvh-1rem)] overflow-x-hidden overflow-y-auto rounded-[18px] border border-border bg-card p-0 text-card-foreground shadow-sm sm:w-[calc(100vw-2rem)] sm:max-h-[calc(100dvh-2rem)] xl:overflow-hidden",
					"max-lg:!left-0 max-lg:!right-auto max-lg:!bottom-0 max-lg:!top-auto max-lg:!w-[100dvw] max-lg:!max-w-[100dvw] max-lg:!translate-x-0 max-lg:!translate-y-0 max-lg:rounded-b-none max-lg:border-x-0 max-lg:border-b-0 max-lg:max-h-[92dvh]",
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

				<button
					type="button"
					className={headerIconBtnBase}
					aria-label="Close"
					data-sub-preview-close
					onClick={() => {
						onClose();
					}}
				>
					<Icon name="tabler:x" size={20} ariaLabel="Close" />
				</button>

				<div
					className={[
						contentPadClass,
						"pt-5 pb-4 sm:pt-[18px] sm:pb-[18px]",
					].join(" ")}
				>
					<div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_auto]">
						<div className="flex min-h-10 min-w-0 items-center pr-12 lg:pr-0">
							<h3 className="text-xl font-[750] leading-7 text-foreground sm:text-[22px]">
								Subscription preview
							</h3>
						</div>

						<div className="grid min-h-11 grid-cols-1 items-center gap-2 sm:flex sm:flex-wrap lg:justify-end">
							<SubscriptionFormatSegmentedControl
								className="w-full min-w-0 sm:w-auto sm:min-w-[260px]"
								hideLegend
								onValueActivate={(nextFormat) => {
									if (loading) return;
									void onFormatChange?.(nextFormat);
								}}
								onValueChange={(nextFormat) => {
									if (loading) return;
									void onFormatChange?.(nextFormat);
								}}
								testId="subscription-preview-format"
								value={format}
							/>
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
					</div>
				</div>

				<div className={[contentPadClass, "pb-[28px]"].join(" ")}>
					{error ? (
						<div className="mb-3 text-sm text-destructive">{error}</div>
					) : null}

					<div
						className={cn(
							"grid gap-4",
							showFieldsPanel
								? "xl:grid-cols-[minmax(0,1fr)_264px]"
								: "lg:grid-cols-1",
						)}
					>
						<div className="min-w-0 space-y-2">
							<SubscriptionContentEditor
								content={content}
								format={format}
								fillHeight={showFieldsPanel}
								loading={loading}
							/>
						</div>

						{showFieldsPanel ? (
							<div className="space-y-3 overflow-hidden rounded-[14px] border border-border bg-muted/35 p-4 xl:h-[508px]">
								<div className="space-y-1">
									<h4 className="text-[13px] font-[750] text-foreground">
										Fields
									</h4>
									<div className={cn("text-[12px]", mutedTextClass)}>
										Click Copy to copy exact value
									</div>
								</div>

								<div className="space-y-4">
									<div className="space-y-1">
										<div
											className={cn("font-mono text-[12px]", mutedTextClass)}
										>
											public-key
										</div>
										<div className="space-y-2">
											<div
												className={fieldValueClass}
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
											className={cn("font-mono text-[12px]", mutedTextClass)}
										>
											short-id
										</div>
										<div className="space-y-2">
											<div
												className={fieldValueClass}
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
											className={cn("font-mono text-[12px]", mutedTextClass)}
										>
											servername
										</div>
										<div className="space-y-2">
											<div
												className={fieldValueClass}
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
											className={fieldCopyButtonClass}
											onClick={async () => {
												await writeClipboard(copyAllFieldsText);
											}}
										>
											Copy all fields
										</button>
									) : null}
								</div>
							</div>
						) : null}
					</div>
				</div>
			</DialogContent>
		</Dialog>
	);
}

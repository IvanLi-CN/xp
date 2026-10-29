import {
	useCallback,
	useEffect,
	useLayoutEffect,
	useRef,
	useState,
} from "react";
import { createPortal } from "react-dom";

import type { EditorView } from "@uiw/react-codemirror";

import type { AdminUserMihomoProfile } from "../api/adminUsers";
import {
	MIHOMO_DRAFT_PERSISTENCE_WARNING,
	MIHOMO_PROFILE_DOCUMENTS,
	type MihomoProfileConflict,
	type MihomoProfileDocumentId,
} from "../hooks/useMihomoProfileDraft";
import { cn } from "../lib/utils";
import type {
	MihomoSaveFeedback,
	MihomoSaveVerification,
} from "../utils/mihomoSaveOutcome";
import { Button, IconButton } from "./Button";
import { Icon } from "./Icon";
import {
	type MihomoImportPreviewRequestFn,
	MihomoSubscriptionImportDialog,
} from "./MihomoSubscriptionImportDialog";
import { YamlCodeEditor } from "./YamlCodeEditor";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogTitle,
} from "./ui/dialog";
import { Sheet, SheetContent, SheetDescription, SheetTitle } from "./ui/sheet";

const DOCUMENT_LABELS: Record<MihomoProfileDocumentId, string> = {
	mixin_yaml: "mixin_yaml",
	extra_proxies_yaml: "extra_proxies_yaml",
	extra_proxy_providers_yaml: "extra_proxy_providers_yaml",
};

const DOCUMENT_PLACEHOLDERS: Record<MihomoProfileDocumentId, string> = {
	mixin_yaml: "Paste Mihomo mixin YAML",
	extra_proxies_yaml: "- name: custom-ss\n  type: ss\n  ...",
	extra_proxy_providers_yaml: "ProviderA:\n  type: http\n  ...",
};

const DOCUMENT_ROWS: Record<MihomoProfileDocumentId, number> = {
	mixin_yaml: 14,
	extra_proxies_yaml: 8,
	extra_proxy_providers_yaml: 8,
};

type MihomoProfileEditorProps = {
	userName: string;
	userId: string;
	profile: AdminUserMihomoProfile | undefined;
	draft: AdminUserMihomoProfile;
	dirty: boolean;
	isSaving: boolean;
	isVerifying?: boolean;
	readOnly?: boolean;
	error: string | null;
	saveFeedback?: MihomoSaveFeedback | null;
	storageError?: string | null;
	conflict?: MihomoProfileConflict | null;
	isLoaded: boolean;
	onChange: (documentId: MihomoProfileDocumentId, value: string) => void;
	onSave: (allowOverwrite?: boolean) => Promise<boolean>;
	onVerify?: () => Promise<boolean>;
	onContinueDraft?: () => void;
	onUseServer?: () => void;
	onExpandAvailabilityChange?: (available: boolean) => void;
	adminToken?: string;
	importAvailable?: boolean;
	previewImport?: MihomoImportPreviewRequestFn;
	portalContainer?: HTMLElement | null;
};

type VisualViewportMetrics = {
	height: number;
	offsetTop: number;
};

function useVisualViewportMetrics(
	enabled: boolean,
): VisualViewportMetrics | undefined {
	const [metrics, setMetrics] = useState<VisualViewportMetrics | null>(null);

	useEffect(() => {
		if (!enabled || typeof window === "undefined") return;
		const viewport = window.visualViewport;
		const update = () =>
			setMetrics({
				height: viewport?.height ?? window.innerHeight,
				offsetTop: viewport?.offsetTop ?? 0,
			});
		update();
		viewport?.addEventListener("resize", update);
		viewport?.addEventListener("scroll", update);
		window.addEventListener("resize", update);
		return () => {
			viewport?.removeEventListener("resize", update);
			viewport?.removeEventListener("scroll", update);
			window.removeEventListener("resize", update);
		};
	}, [enabled]);

	return metrics ?? undefined;
}

function documentIsDirty(
	documentId: MihomoProfileDocumentId,
	draft: AdminUserMihomoProfile,
	profile: AdminUserMihomoProfile | undefined,
): boolean {
	return profile !== undefined && draft[documentId] !== profile[documentId];
}

function statusLabel(
	readOnly: boolean,
	dirty: boolean,
): "Read-only" | "Unsaved changes" | "Saved" {
	if (readOnly) return "Read-only";
	return dirty ? "Unsaved changes" : "Saved";
}

function feedbackCopy(verification: MihomoSaveVerification): {
	title: string;
	description: string;
} {
	switch (verification) {
		case "matches":
			return {
				title: "Read-back matches your draft",
				description:
					"The saved profile currently has the same three files. " +
					"The original write response was not confirmed, so keep the " +
					"diagnostic details for follow-up.",
			};
		case "different":
			return {
				title: "Save result is still unknown",
				description:
					"The saved profile differs from your draft. Your draft is preserved; " +
					"check again or review it before retrying.",
			};
		case "unavailable":
			return {
				title: "Save result is still unknown",
				description:
					"The read-back could not be completed. Your draft is preserved; " +
					"check again when the backend is reachable.",
			};
	}
}

function MihomoSaveFeedbackPanel({
	feedback,
	error,
	retryAcknowledged,
	isVerifying,
	onVerify,
}: {
	feedback: MihomoSaveFeedback | null | undefined;
	error: string | null;
	retryAcknowledged: boolean;
	isVerifying: boolean;
	onVerify?: () => Promise<boolean>;
}) {
	const copy = feedback ? feedbackCopy(feedback.verification) : null;
	const details = feedback
		? [
				["Error code", feedback.diagnostics.code],
				["Dispatch state", feedback.diagnostics.dispatchState],
				["Cause", feedback.diagnostics.cause],
				["Target node", feedback.diagnostics.targetNodeId],
				["Support ID", feedback.diagnostics.supportId],
			].filter(([, value]) => value)
		: [];
	const showError =
		error !== null && (!feedback || error === MIHOMO_DRAFT_PERSISTENCE_WARNING);
	const showRetryWarning =
		retryAcknowledged && feedback?.kind === "outcome_unknown";
	if (!feedback && !showError && !showRetryWarning) return null;

	return (
		<div
			className={
				"pointer-events-none fixed inset-x-4 bottom-4 z-[70] flex justify-end " +
				"sm:inset-x-auto sm:right-4 sm:w-[min(32rem,calc(100vw-2rem))]"
			}
			data-mihomo-feedback-overlay
		>
			<div
				className={cn(
					"pointer-events-auto max-h-[min(60vh,32rem)] w-full overflow-y-auto",
					"rounded-xl border border-border bg-popover/95 p-4 text-popover-foreground",
					"shadow-sm",
				)}
			>
				{feedback && copy ? (
					<output
						className="block space-y-2"
						aria-live="polite"
						aria-atomic="true"
					>
						<div className="flex items-start gap-2">
							<Icon
								name="tabler:alert-triangle"
								className="mt-1 size-4 shrink-0 text-foreground"
								ariaLabel="Save result unknown"
							/>
							<div className="min-w-0 space-y-1">
								<div className="font-medium">{copy.title}</div>
								<div className="text-sm text-muted-foreground">
									{copy.description}
								</div>
							</div>
						</div>
						<div className="text-xs text-foreground">
							A manual retry may overwrite changes made elsewhere in the
							meantime.
						</div>
						{details.length > 0 ? (
							<details className="text-xs">
								<summary className="cursor-pointer select-none font-medium">
									Technical details
								</summary>
								<dl className="mt-2 grid gap-1 sm:grid-cols-[auto_1fr] sm:gap-x-3">
									{details.map(([label, value]) => (
										<div key={label} className="contents">
											<dt className="text-muted-foreground">{label}</dt>
											<dd className="break-all font-mono">{value}</dd>
										</div>
									))}
								</dl>
							</details>
						) : null}
						{onVerify ? (
							<Button
								variant="outline"
								size="sm"
								loading={isVerifying}
								disabled={isVerifying}
								onClick={() => void onVerify()}
							>
								Check again
							</Button>
						) : null}
					</output>
				) : null}
				{showError ? (
					<div
						className="xp-alert xp-alert-error mt-3 flex items-start gap-2 px-3 py-2 text-xs"
						role="alert"
					>
						<Icon
							name="tabler:alert-circle"
							className="mt-1 size-4 shrink-0"
							ariaLabel="Save error"
						/>
						<span className="min-w-0 break-words">{error}</span>
					</div>
				) : null}
				{showRetryWarning ? (
					<div className="mt-3 text-xs text-foreground" role="alert">
						Click Retry save again to confirm. This may overwrite changes made
						elsewhere.
					</div>
				) : null}
			</div>
		</div>
	);
}

export function MihomoProfileEditor({
	userName,
	userId,
	profile,
	draft,
	dirty,
	isSaving,
	isVerifying = false,
	readOnly = false,
	error,
	saveFeedback,
	storageError = null,
	conflict = null,
	isLoaded,
	onChange,
	onSave,
	onVerify,
	onContinueDraft,
	onUseServer,
	onExpandAvailabilityChange,
	adminToken = "",
	importAvailable = false,
	previewImport,
	portalContainer,
}: MihomoProfileEditorProps) {
	const [expanded, setExpanded] = useState(false);
	const [importOpen, setImportOpen] = useState(false);
	const [filesOpen, setFilesOpen] = useState(false);
	const [selectedDocument, setSelectedDocument] =
		useState<MihomoProfileDocumentId>("mixin_yaml");
	const [portalHost, setPortalHost] = useState<HTMLElement | null>(null);
	const [expandedFeedbackMount, setExpandedFeedbackMount] =
		useState<HTMLElement | null>(null);
	const [filesFeedbackMount, setFilesFeedbackMount] =
		useState<HTMLElement | null>(null);
	const inlineMount = useRef<HTMLDivElement>(null);
	const expandedMount = useRef<HTMLDivElement>(null);
	const entryButton = useRef<HTMLDivElement>(null);
	const filesTrigger = useRef<HTMLDivElement>(null);
	const savedScrollY = useRef(0);
	const views = useRef<Partial<Record<MihomoProfileDocumentId, EditorView>>>(
		{},
	);
	const viewportMetrics = useVisualViewportMetrics(expanded);
	const currentStatus = statusLabel(readOnly, dirty);
	const canImport = importAvailable && !readOnly && isLoaded && !isVerifying;
	const [retryAcknowledged, setRetryAcknowledged] = useState(false);
	const needsRetryWarning = saveFeedback?.kind === "outcome_unknown" && dirty;

	useEffect(() => {
		if (dirty || saveFeedback) setRetryAcknowledged(false);
	}, [dirty, saveFeedback]);

	const handleSave = useCallback(() => {
		if (needsRetryWarning && !retryAcknowledged) {
			setRetryAcknowledged(true);
			return;
		}
		const allowOverwrite = needsRetryWarning && retryAcknowledged;
		setRetryAcknowledged(false);
		void onSave(allowOverwrite);
	}, [needsRetryWarning, onSave, retryAcknowledged]);
	const handleVerify = useCallback(async () => {
		if (!onVerify) return false;
		try {
			return await onVerify();
		} finally {
			setRetryAcknowledged(false);
		}
	}, [onVerify]);

	const saveLabel = needsRetryWarning ? "Retry save" : "Save configuration";
	const conflictNotice = conflict ? (
		<div
			className={cn(
				"fixed inset-x-4 bottom-20 z-[60] flex-col px-4 py-3 shadow-sm",
				"xp-alert xp-alert-warning sm:left-auto sm:w-[min(32rem,calc(100vw-2rem))]",
			)}
			role="alert"
			aria-live="polite"
		>
			<div className="flex items-start gap-2">
				<Icon
					name="tabler:alert-triangle"
					className="mt-1 size-4 shrink-0"
					ariaLabel="Profile conflict"
				/>
				<div className="min-w-0 font-medium">
					The server profile changed. Your draft is preserved.
				</div>
			</div>
			<details className="text-xs text-muted-foreground">
				<summary className="cursor-pointer font-medium text-foreground">
					View differences
				</summary>
				<ul className="mt-2 space-y-1">
					{MIHOMO_PROFILE_DOCUMENTS.map((documentId) =>
						conflict.draftBaseline[documentId] ===
						profile?.[documentId] ? null : (
							<li key={documentId}>{documentId}: server baseline changed</li>
						),
					)}
				</ul>
			</details>
			<div className="flex flex-wrap gap-2">
				<Button
					size="sm"
					variant="outline"
					disabled={isSaving || !onContinueDraft}
					onClick={onContinueDraft}
				>
					Continue editing draft
				</Button>
				<Button
					size="sm"
					variant="ghost"
					disabled={isSaving || !onUseServer}
					onClick={onUseServer}
				>
					Use server version
				</Button>
			</div>
		</div>
	) : null;

	useEffect(() => {
		if (typeof document === "undefined") return;
		const host = document.createElement("div");
		host.dataset.mihomoEditorHost = userId;
		host.className =
			"mihomo-editor-host flex h-full min-h-0 min-w-0 max-w-full flex-col";
		setPortalHost(host);
		return () => host.remove();
	}, [userId]);

	useEffect(() => {
		onExpandAvailabilityChange?.(isLoaded);
	}, [isLoaded, onExpandAvailabilityChange]);

	useEffect(() => {
		if (userId.length === 0) return;
		setSelectedDocument("mixin_yaml");
		setExpanded(false);
		setImportOpen(false);
		setFilesOpen(false);
	}, [userId]);

	useLayoutEffect(() => {
		if (!portalHost) return;
		const moveHost = () => {
			const target = expanded ? expandedMount.current : inlineMount.current;
			if (!target) return;
			target.replaceChildren(portalHost);
			for (const view of Object.values(views.current)) view?.requestMeasure();
		};
		moveHost();
		const frame = requestAnimationFrame(moveHost);
		return () => cancelAnimationFrame(frame);
	}, [expanded, portalHost]);

	useEffect(() => {
		if (!expanded) return;
		const activeView = views.current[selectedDocument];
		if (!activeView) return;
		const frame = requestAnimationFrame(() => {
			activeView.requestMeasure();
			activeView.focus();
		});
		return () => cancelAnimationFrame(frame);
	}, [expanded, selectedDocument]);

	const openExpanded = useCallback(() => {
		if (!isLoaded) return;
		savedScrollY.current = window.scrollY;
		setExpanded(true);
	}, [isLoaded]);

	const openImport = useCallback(() => {
		if (!canImport) return;
		setImportOpen(true);
	}, [canImport]);

	const closeExpanded = useCallback(() => {
		setFilesOpen(false);
		setExpanded(false);
		requestAnimationFrame(() => {
			window.scrollTo({ top: savedScrollY.current, behavior: "auto" });
			entryButton.current?.querySelector<HTMLButtonElement>("button")?.focus();
		});
	}, []);

	const closeFiles = useCallback(() => {
		setFilesOpen(false);
		requestAnimationFrame(() => {
			filesTrigger.current?.querySelector<HTMLButtonElement>("button")?.focus();
		});
	}, []);

	const handleEscape = useCallback(
		(event: KeyboardEvent) => {
			if (event.defaultPrevented || filesOpen) return;
			event.preventDefault();
			closeExpanded();
		},
		[closeExpanded, filesOpen],
	);

	const renderDocument = (documentId: MihomoProfileDocumentId) => (
		<div
			key={documentId}
			className={cn(
				"min-w-0",
				expanded && documentId !== selectedDocument && "hidden",
				expanded && "h-full",
			)}
			data-mihomo-document={documentId}
		>
			{documentId === "extra_proxies_yaml" && !expanded ? (
				<div className="mb-2 flex items-center justify-between gap-3">
					<span className="text-sm font-medium text-foreground">
						{DOCUMENT_LABELS[documentId]}
					</span>
					{canImport ? (
						<Button
							variant="outline"
							size="sm"
							iconLeft={<Icon name="tabler:download" />}
							onClick={openImport}
						>
							Import nodes
						</Button>
					) : null}
				</div>
			) : null}
			<YamlCodeEditor
				label={DOCUMENT_LABELS[documentId]}
				value={draft[documentId]}
				onChange={(value) => onChange(documentId, value)}
				placeholder={DOCUMENT_PLACEHOLDERS[documentId]}
				minRows={DOCUMENT_ROWS[documentId]}
				readOnly={readOnly || isSaving || isVerifying || !isLoaded}
				fillHeight={expanded}
				preserveEditorStateOnValueChange
				hideLabel={expanded || documentId === "extra_proxies_yaml"}
				onCreateEditor={(view) => {
					views.current[documentId] = view;
				}}
				showShortcutHint={documentId === "mixin_yaml" && !expanded}
			/>
		</div>
	);

	const fileList = (
		<nav className="flex min-h-0 flex-col gap-1" aria-label="Mihomo files">
			{MIHOMO_PROFILE_DOCUMENTS.map((documentId) => {
				const itemDirty = documentIsDirty(documentId, draft, profile);
				return (
					<button
						key={documentId}
						type="button"
						aria-current={selectedDocument === documentId ? "page" : undefined}
						className={cn(
							"group flex min-h-10 w-full items-center gap-2.5 rounded-md",
							"px-3 py-2 text-left text-sm transition-colors",
							"hover:bg-accent hover:text-accent-foreground",
							"focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
							selectedDocument === documentId &&
								"bg-primary/12 text-primary hover:bg-primary/16 hover:text-primary",
						)}
						onClick={() => {
							setSelectedDocument(documentId);
							closeFiles();
						}}
					>
						<Icon
							name="tabler:file-code-2"
							className="size-4 shrink-0 text-muted-foreground group-hover:text-current"
							ariaLabel=""
						/>
						<span className="min-w-0 flex-1 truncate font-mono text-[12px]">
							{documentId}
						</span>
						{itemDirty ? (
							<span
								className="flex shrink-0 items-center gap-1 text-[11px] text-warning-foreground"
								aria-label="Unsaved changes"
							>
								<span
									className="size-1.5 rounded-full bg-warning-foreground"
									aria-hidden="true"
								/>
								Unsaved
							</span>
						) : null}
					</button>
				);
			})}
		</nav>
	);

	const editorContent = portalHost
		? createPortal(
				MIHOMO_PROFILE_DOCUMENTS.map((documentId) =>
					renderDocument(documentId),
				),
				portalHost,
			)
		: null;
	const feedbackPortalContainer =
		(filesOpen && filesFeedbackMount) ||
		(expanded && expandedFeedbackMount) ||
		portalContainer?.ownerDocument.body ||
		(typeof document !== "undefined" ? document.body : null);
	const feedbackError = error ?? storageError;
	const feedbackContent =
		feedbackPortalContainer &&
		(saveFeedback || feedbackError || (needsRetryWarning && retryAcknowledged))
			? createPortal(
					<MihomoSaveFeedbackPanel
						feedback={saveFeedback}
						error={feedbackError}
						retryAcknowledged={retryAcknowledged}
						isVerifying={isVerifying}
						onVerify={onVerify ? handleVerify : undefined}
					/>,
					feedbackPortalContainer,
				)
			: null;

	return (
		<>
			{conflictNotice}
			<div
				className="flex items-center justify-between gap-3"
				aria-busy={!isLoaded}
				data-mihomo-editor-loaded={isLoaded ? "true" : "false"}
			>
				<div className="min-w-0">
					<div className="font-medium text-sm">
						Mihomo mixin config (per user)
					</div>
					<div className="text-xs text-muted-foreground">
						{currentStatus === "Unsaved changes"
							? "Unsaved configuration changes"
							: currentStatus === "Read-only"
								? "Read-only profile"
								: "Saved configuration"}
					</div>
				</div>
				<div ref={entryButton}>
					<IconButton
						label="Expand editor"
						tooltip="Expand editor"
						disabled={!isLoaded}
						onClick={openExpanded}
					>
						<Icon name="tabler:arrows-maximize" ariaLabel="Expand editor" />
					</IconButton>
				</div>
			</div>
			<div ref={inlineMount} className="min-w-0" />
			<div className="flex items-center justify-between gap-3">
				<span className="text-xs text-muted-foreground">
					{readOnly ? "Read-only profile" : "Changes apply to all three files."}
				</span>
				<div className="flex items-center gap-2">
					<Button
						loading={isSaving || isVerifying}
						disabled={
							readOnly ||
							!dirty ||
							!isLoaded ||
							isVerifying ||
							Boolean(conflict)
						}
						iconLeft={<Icon name="tabler:device-floppy" />}
						onClick={handleSave}
					>
						{saveLabel}
					</Button>
				</div>
			</div>

			<Dialog
				open={expanded}
				onOpenChange={(open) => (open ? openExpanded() : closeExpanded())}
			>
				<DialogContent
					showCloseButton={false}
					className={cn(
						"inset-0 left-0 top-0 h-[100dvh] w-full max-w-none",
						"translate-x-0 translate-y-0 gap-0 rounded-none border-0 p-0",
					)}
					onEscapeKeyDown={handleEscape}
					onOpenAutoFocus={(event) => event.preventDefault()}
					onPointerDownOutside={(event) => event.preventDefault()}
					onInteractOutside={(event) => event.preventDefault()}
					style={
						viewportMetrics
							? {
									top: `${viewportMetrics.offsetTop}px`,
									bottom: "auto",
									height: `${viewportMetrics.height}px`,
								}
							: undefined
					}
				>
					<div
						data-testid="mihomo-workspace-surface"
						className="flex min-h-0 min-w-0 flex-1 flex-col bg-background text-foreground"
						style={{
							height: viewportMetrics
								? `${viewportMetrics.height}px`
								: "100dvh",
						}}
					>
						<div
							className={cn(
								"flex min-h-16 shrink-0 items-center gap-3 border-b border-border bg-card",
								"px-4 pb-2 pt-[env(safe-area-inset-top)] sm:px-6",
							)}
						>
							<div className="flex min-w-0 flex-1 items-center gap-3">
								<div
									className={cn(
										"flex size-8 shrink-0 items-center justify-center rounded-md",
										"bg-primary/12 text-primary",
									)}
								>
									<Icon name="tabler:braces" ariaLabel="Mihomo profile" />
								</div>
								<div className="min-w-0">
									<div className="flex min-w-0 items-center gap-2">
										<DialogTitle className="truncate text-sm font-semibold">
											{userName}
										</DialogTitle>
										<span className="hidden shrink-0 text-[11px] text-muted-foreground sm:inline">
											Mihomo profile
										</span>
									</div>
									<DialogDescription className="mt-1 flex min-w-0 items-center gap-2 text-xs">
										<span className="truncate font-mono text-muted-foreground">
											{selectedDocument}
										</span>
										<span className="text-border" aria-hidden="true">
											/
										</span>
										<span
											className={cn(
												"shrink-0",
												currentStatus === "Unsaved changes"
													? "text-warning"
													: "text-muted-foreground",
											)}
										>
											{currentStatus}
										</span>
									</DialogDescription>
								</div>
							</div>
							<div className="flex shrink-0 items-center gap-2">
								{canImport ? (
									<>
										<Button
											className="hidden sm:inline-flex"
											variant="outline"
											size="sm"
											iconLeft={<Icon name="tabler:download" />}
											onClick={openImport}
										>
											Import nodes
										</Button>
										<IconButton
											className="sm:hidden"
											label="Import nodes"
											tooltip="Import nodes"
											onClick={openImport}
										>
											<Icon name="tabler:download" ariaLabel="Import nodes" />
										</IconButton>
									</>
								) : null}
								<div ref={filesTrigger} className="md:hidden">
									<IconButton
										label="Open files"
										tooltip="Open files"
										onClick={() => setFilesOpen(true)}
									>
										<Icon name="tabler:files" ariaLabel="Open files" />
									</IconButton>
								</div>
								<Button
									className="hidden sm:inline-flex"
									size="sm"
									loading={isSaving || isVerifying}
									disabled={
										readOnly ||
										!dirty ||
										!isLoaded ||
										isVerifying ||
										Boolean(conflict)
									}
									iconLeft={<Icon name="tabler:device-floppy" />}
									onClick={handleSave}
								>
									{saveLabel}
								</Button>
								<IconButton
									className="sm:hidden"
									label={saveLabel}
									tooltip={saveLabel}
									loading={isSaving || isVerifying}
									disabled={
										readOnly ||
										!dirty ||
										!isLoaded ||
										isVerifying ||
										Boolean(conflict)
									}
									onClick={handleSave}
								>
									<Icon name="tabler:device-floppy" ariaLabel={saveLabel} />
								</IconButton>
								<IconButton
									label="Exit expanded editor"
									tooltip="Exit expanded editor"
									onClick={closeExpanded}
								>
									<Icon
										name="tabler:arrows-minimize"
										ariaLabel="Exit expanded editor"
									/>
								</IconButton>
							</div>
						</div>
						<div className="flex min-h-0 flex-1 bg-muted/20">
							<aside className="hidden w-64 shrink-0 flex-col border-r border-border bg-card/45 md:flex">
								<div
									className={cn(
										"flex h-11 shrink-0 items-center justify-between",
										"border-b border-border px-4",
									)}
								>
									<span
										className={cn(
											"text-[11px] font-semibold uppercase",
											"tracking-[0.12em] text-muted-foreground",
										)}
									>
										Files
									</span>
									<span className="text-[11px] tabular-nums text-muted-foreground">
										{MIHOMO_PROFILE_DOCUMENTS.length} files
									</span>
								</div>
								<div className="min-h-0 flex-1 overflow-y-auto p-2">
									{fileList}
								</div>
							</aside>
							<main className="flex min-w-0 flex-1 flex-col overflow-hidden bg-background">
								<div
									className={cn(
										"flex min-h-11 shrink-0 items-center justify-between gap-3",
										"border-b border-border px-4",
									)}
								>
									<div className="flex min-w-0 items-center gap-2">
										<Icon
											name="tabler:file-code-2"
											className="size-4 shrink-0 text-primary"
											ariaLabel="Current file"
										/>
										<span className="truncate font-mono text-xs font-medium">
											{selectedDocument}
										</span>
									</div>
									<div className="flex shrink-0 items-center gap-3 text-[11px] text-muted-foreground">
										<span className="hidden sm:inline">YAML</span>
										{readOnly ? <span>Read-only</span> : null}
									</div>
								</div>
								<div
									ref={expandedMount}
									className="min-h-0 flex-1 overflow-hidden"
								/>
								<div
									className={cn(
										"flex min-h-7 shrink-0 items-center justify-between gap-3",
										"border-t border-border px-3 text-[11px] text-muted-foreground",
									)}
								>
									<span className="flex min-w-0 items-center gap-1.5 truncate">
										<Icon name="tabler:code" size={14} ariaLabel="Editor" />
										<span className="truncate">Mihomo profile</span>
									</span>
									<span className="shrink-0">{currentStatus}</span>
								</div>
							</main>
						</div>
					</div>
					<div
						ref={setExpandedFeedbackMount}
						className="pointer-events-none fixed inset-0"
					/>
				</DialogContent>
			</Dialog>

			<Sheet
				open={filesOpen}
				onOpenChange={(open) => (open ? setFilesOpen(true) : closeFiles())}
			>
				<SheetContent
					side="left"
					className="z-[60] w-[min(86vw,20rem)] border-r border-border bg-card p-0"
					style={{ paddingTop: "env(safe-area-inset-top)" }}
					onCloseAutoFocus={(event) => {
						event.preventDefault();
						requestAnimationFrame(() => {
							filesTrigger.current
								?.querySelector<HTMLButtonElement>("button")
								?.focus();
						});
					}}
				>
					<div className="flex h-14 items-center border-b border-border px-4">
						<div>
							<SheetTitle className="text-sm">Files</SheetTitle>
							<SheetDescription className="sr-only">
								Select a Mihomo profile document.
							</SheetDescription>
						</div>
					</div>
					<div className="min-h-0 flex-1 overflow-y-auto p-2">{fileList}</div>
					<div
						ref={setFilesFeedbackMount}
						className="pointer-events-none fixed inset-0"
					/>
				</SheetContent>
			</Sheet>
			<MihomoSubscriptionImportDialog
				key={userId}
				open={importOpen}
				onOpenChange={setImportOpen}
				adminToken={adminToken}
				userId={userId}
				existingExtraProxiesYaml={draft.extra_proxies_yaml}
				readOnly={readOnly}
				onAdd={(nextValue) => onChange("extra_proxies_yaml", nextValue)}
				previewRequest={previewImport}
				portalContainer={portalContainer}
			/>
			{feedbackContent}
			{editorContent}
		</>
	);
}

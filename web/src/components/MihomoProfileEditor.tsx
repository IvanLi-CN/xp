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
	MIHOMO_PROFILE_DOCUMENTS,
	type MihomoProfileDocumentId,
} from "../hooks/useMihomoProfileDraft";
import { cn } from "../lib/utils";
import { Button, IconButton } from "./Button";
import { Icon } from "./Icon";
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
	readOnly?: boolean;
	error: string | null;
	isLoaded: boolean;
	onChange: (documentId: MihomoProfileDocumentId, value: string) => void;
	onSave: () => Promise<boolean>;
	onExpandAvailabilityChange?: (available: boolean) => void;
};

function useVisualViewportHeight(enabled: boolean): string | undefined {
	const [height, setHeight] = useState<number | null>(null);

	useEffect(() => {
		if (!enabled || typeof window === "undefined") return;
		const viewport = window.visualViewport;
		const update = () => setHeight(viewport?.height ?? window.innerHeight);
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

	return height === null ? undefined : `${height}px`;
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

export function MihomoProfileEditor({
	userName,
	userId,
	profile,
	draft,
	dirty,
	isSaving,
	readOnly = false,
	error,
	isLoaded,
	onChange,
	onSave,
	onExpandAvailabilityChange,
}: MihomoProfileEditorProps) {
	const [expanded, setExpanded] = useState(false);
	const [filesOpen, setFilesOpen] = useState(false);
	const [selectedDocument, setSelectedDocument] =
		useState<MihomoProfileDocumentId>("mixin_yaml");
	const [portalHost, setPortalHost] = useState<HTMLElement | null>(null);
	const inlineMount = useRef<HTMLDivElement>(null);
	const expandedMount = useRef<HTMLDivElement>(null);
	const entryButton = useRef<HTMLDivElement>(null);
	const filesTrigger = useRef<HTMLDivElement>(null);
	const savedScrollY = useRef(0);
	const views = useRef<Partial<Record<MihomoProfileDocumentId, EditorView>>>(
		{},
	);
	const viewportHeight = useVisualViewportHeight(expanded);
	const currentStatus = statusLabel(readOnly, dirty);

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
			<YamlCodeEditor
				label={DOCUMENT_LABELS[documentId]}
				value={draft[documentId]}
				onChange={(value) => onChange(documentId, value)}
				placeholder={DOCUMENT_PLACEHOLDERS[documentId]}
				minRows={DOCUMENT_ROWS[documentId]}
				readOnly={readOnly || isSaving || !isLoaded}
				fillHeight={expanded}
				preserveEditorStateOnValueChange
				hideLabel={expanded}
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

	return (
		<>
			<div className="flex items-center justify-between gap-3">
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
			{error ? (
				<div className="xp-alert xp-alert-error px-4 py-2">{error}</div>
			) : null}
			<div className="flex items-center justify-between gap-3">
				<span className="text-xs text-muted-foreground">
					{readOnly ? "Read-only profile" : "Changes apply to all three files."}
				</span>
				<Button
					loading={isSaving}
					disabled={readOnly || !dirty || !isLoaded}
					iconLeft={<Icon name="tabler:device-floppy" />}
					onClick={() => void onSave()}
				>
					Save configuration
				</Button>
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
				>
					<div
						className="flex min-h-0 flex-1 flex-col bg-background text-foreground"
						style={{ height: viewportHeight ?? "100dvh" }}
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
								<div ref={filesTrigger} className="md:hidden">
									<Button
										variant="outline"
										size="sm"
										iconLeft={<Icon name="tabler:files" />}
										onClick={() => setFilesOpen(true)}
									>
										Files
									</Button>
								</div>
								<Button
									className="hidden sm:inline-flex"
									size="sm"
									loading={isSaving}
									disabled={readOnly || !dirty || !isLoaded}
									iconLeft={<Icon name="tabler:device-floppy" />}
									onClick={() => void onSave()}
								>
									Save configuration
								</Button>
								<IconButton
									className="sm:hidden"
									label="Save configuration"
									tooltip="Save configuration"
									loading={isSaving}
									disabled={readOnly || !dirty || !isLoaded}
									onClick={() => void onSave()}
								>
									<Icon
										name="tabler:device-floppy"
										ariaLabel="Save configuration"
									/>
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
								{error ? (
									<div
										className="xp-alert xp-alert-error mx-3 mt-3 shrink-0 px-3 py-2 text-xs sm:mx-4"
										role="alert"
									>
										<Icon
											name="tabler:alert-circle"
											className="mt-0.5 size-4 shrink-0"
											ariaLabel="Save error"
										/>
										<span className="min-w-0 break-words">{error}</span>
									</div>
								) : null}
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
				</SheetContent>
			</Sheet>
			{editorContent}
		</>
	);
}

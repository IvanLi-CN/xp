import { useCallback, useEffect, useRef, useState } from "react";

import {
	type AdminMihomoImportPreview,
	previewAdminUserMihomoImport,
} from "../api/adminUsers";
import { cn } from "../lib/utils";
import { Button } from "./Button";
import { Icon } from "./Icon";
import {
	Sheet,
	SheetContent,
	SheetDescription,
	SheetFooter,
	SheetHeader,
	SheetTitle,
} from "./ui/sheet";

export type MihomoImportPreviewRequestFn = (
	source: string,
	existingExtraProxiesYaml: string,
	signal?: AbortSignal,
) => Promise<AdminMihomoImportPreview>;

type MihomoSubscriptionImportDialogProps = {
	open: boolean;
	onOpenChange: (open: boolean) => void;
	adminToken: string;
	userId: string;
	existingExtraProxiesYaml: string;
	readOnly?: boolean;
	onAdd: (nextExtraProxiesYaml: string) => void;
	previewRequest?: MihomoImportPreviewRequestFn;
	portalContainer?: HTMLElement | null;
};

const SOURCE_LABELS: Record<
	AdminMihomoImportPreview["detected_source"],
	string
> = {
	remote_url: "Remote URL",
	raw: "Node URI",
	base64: "Base64",
	mihomo_yaml: "Mihomo YAML",
};

function statusLabel(
	status: AdminMihomoImportPreview["items"][number]["status"],
): string {
	return status === "added"
		? "Added"
		: status === "skipped"
			? "Skipped"
			: "Failed";
}

function statusClass(
	status: AdminMihomoImportPreview["items"][number]["status"],
): string {
	return status === "added"
		? "bg-success/12 text-success"
		: status === "skipped"
			? "bg-muted text-muted-foreground"
			: "bg-destructive/12 text-destructive";
}

export function MihomoSubscriptionImportDialog({
	open,
	onOpenChange,
	adminToken,
	userId,
	existingExtraProxiesYaml,
	readOnly = false,
	onAdd,
	previewRequest,
	portalContainer,
}: MihomoSubscriptionImportDialogProps) {
	const [source, setSource] = useState("");
	const [preview, setPreview] = useState<AdminMihomoImportPreview | null>(null);
	const [previewBaseline, setPreviewBaseline] = useState<string | null>(null);
	const [isLoading, setIsLoading] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const abortRef = useRef<AbortController | null>(null);
	const requestIdRef = useRef(0);
	const previewFingerprint = `${userId}\u0000${existingExtraProxiesYaml}`;

	const cancelRequest = useCallback(() => {
		requestIdRef.current += 1;
		abortRef.current?.abort();
		abortRef.current = null;
		setIsLoading(false);
	}, []);

	useEffect(() => {
		if (open) return;
		cancelRequest();
		setSource("");
		setPreview(null);
		setPreviewBaseline(null);
		setError(null);
	}, [cancelRequest, open]);

	useEffect(() => () => cancelRequest(), [cancelRequest]);

	const close = useCallback(() => {
		cancelRequest();
		onOpenChange(false);
	}, [cancelRequest, onOpenChange]);

	const handlePreview = useCallback(async () => {
		const trimmedSource = source.trim();
		if (!trimmedSource) {
			setError("Paste a subscription URL or node content first.");
			setPreview(null);
			setPreviewBaseline(null);
			return;
		}

		cancelRequest();
		const controller = new AbortController();
		const requestId = requestIdRef.current;
		abortRef.current = controller;
		setIsLoading(true);
		setError(null);
		setPreview(null);
		setPreviewBaseline(null);

		try {
			const request =
				previewRequest ??
				((nextSource, existing, signal) =>
					previewAdminUserMihomoImport(
						adminToken,
						userId,
						{
							source: nextSource,
							existing_extra_proxies_yaml: existing,
						},
						signal,
					));
			const nextPreview = await request(
				trimmedSource,
				existingExtraProxiesYaml,
				controller.signal,
			);
			if (requestId !== requestIdRef.current) return;
			setPreview(nextPreview);
			setPreviewBaseline(previewFingerprint);
		} catch (cause) {
			if (requestId !== requestIdRef.current || controller.signal.aborted)
				return;
			setError(cause instanceof Error ? cause.message : "Preview failed.");
		} finally {
			if (requestId === requestIdRef.current) {
				abortRef.current = null;
				setIsLoading(false);
			}
		}
	}, [
		adminToken,
		cancelRequest,
		existingExtraProxiesYaml,
		previewFingerprint,
		previewRequest,
		source,
		userId,
	]);

	const isStale =
		preview !== null &&
		previewBaseline !== null &&
		previewBaseline !== previewFingerprint;
	const canAdd =
		!readOnly &&
		!isLoading &&
		preview !== null &&
		!isStale &&
		preview.counts.added > 0 &&
		preview.added_proxies_yaml.trim().length > 0;

	const handleAdd = useCallback(() => {
		if (!preview || isStale || !canAdd) return;
		const separator =
			existingExtraProxiesYaml.length > 0 &&
			!existingExtraProxiesYaml.endsWith("\n")
				? "\n"
				: "";
		onAdd(
			`${existingExtraProxiesYaml}${separator}${preview.added_proxies_yaml}`,
		);
		close();
	}, [canAdd, close, existingExtraProxiesYaml, isStale, onAdd, preview]);

	return (
		<Sheet open={open} onOpenChange={(nextOpen) => !nextOpen && close()}>
			<SheetContent
				side="bottom"
				data-testid="mihomo-import-panel"
				portalContainer={portalContainer}
				aria-busy={isLoading}
				className={cn(
					"grid h-auto max-h-[min(46rem,calc(100dvh-1rem))] w-full max-w-none",
					"grid-rows-[auto_minmax(0,1fr)_auto] gap-0 overflow-hidden",
					"rounded-t-2xl border-x-0 border-b-0 p-0",
					"sm:bottom-auto sm:left-1/2 sm:right-auto sm:top-1/2",
					"sm:max-h-[min(46rem,calc(100dvh-2rem))]",
					"sm:w-[calc(100vw-2rem)] sm:max-w-2xl",
					"sm:-translate-x-1/2 sm:-translate-y-1/2",
					"sm:rounded-2xl sm:border",
				)}
			>
				<output className="sr-only" aria-live="polite" aria-atomic="true">
					{isLoading
						? "Previewing imported nodes."
						: preview
							? [
									"Preview complete.",
									`${preview.counts.added} added,`,
									`${preview.counts.skipped} skipped,`,
									`${preview.counts.failed} failed,`,
									`${preview.counts.warnings} warning${preview.counts.warnings === 1 ? "" : "s"}.`,
								].join(" ")
							: ""}
				</output>
				<SheetHeader className="shrink-0 border-b border-border px-5 pb-4 pt-2 sm:px-6 sm:py-4">
					<div
						className="mx-auto mb-1 mt-0.5 h-1 w-10 rounded-full bg-muted-foreground/30 sm:hidden"
						aria-hidden="true"
					/>
					<SheetTitle className="flex items-center gap-2 pr-14 text-base">
						<Icon
							name="tabler:download"
							className="size-5 text-primary"
							ariaLabel=""
						/>
						Import Mihomo nodes
					</SheetTitle>
					<SheetDescription>
						Paste a URL, node URI, Base64 text, or Mihomo YAML to preview valid
						nodes.
					</SheetDescription>
				</SheetHeader>

				<div className="min-h-0 flex-1 space-y-4 overflow-y-auto px-5 py-4 sm:px-6">
					<div className="space-y-2">
						<label
							className="text-xs font-medium text-foreground"
							htmlFor="mihomo-import-source"
						>
							Subscription or node content
						</label>
						<textarea
							id="mihomo-import-source"
							className={cn(
								"min-h-32 w-full resize-y rounded-md border border-input bg-background px-3 py-2",
								"font-mono text-xs leading-5 shadow-sm outline-none placeholder:text-muted-foreground",
								"focus-visible:ring-2 focus-visible:ring-ring",
							)}
							placeholder="ss://... or vmess://... or paste Base64 / Mihomo YAML"
							value={source}
							disabled={isLoading}
							onChange={(event) => {
								setSource(event.target.value);
								setError(null);
								setPreview(null);
								setPreviewBaseline(null);
							}}
						/>
					</div>

					{error ? (
						<div
							className="xp-alert xp-alert-error px-3 py-2 text-xs"
							role="alert"
						>
							<Icon
								name="tabler:alert-circle"
								className="size-4 shrink-0"
								ariaLabel=""
							/>
							<span className="min-w-0 break-words">{error}</span>
						</div>
					) : null}

					{isStale ? (
						<output className="xp-alert xp-alert-warning px-3 py-2 text-xs">
							<Icon
								name="tabler:refresh-alert"
								className="size-4 shrink-0"
								ariaLabel=""
							/>
							<span>
								The profile changed while this preview was open. Preview again
								before adding nodes.
							</span>
						</output>
					) : null}

					{preview ? (
						<section className="space-y-3" aria-label="Import preview">
							<div className="flex flex-wrap items-center gap-2">
								<div className="flex items-center gap-2 text-sm font-medium">
									<span>Preview report</span>
									<span className="rounded-full bg-primary/10 px-2 py-0.5 text-[11px] text-primary">
										{SOURCE_LABELS[preview.detected_source]}
									</span>
								</div>
							</div>
							<dl
								aria-label="Import counts"
								data-testid="mihomo-import-summary"
								className={cn(
									"flex flex-wrap items-center gap-x-4 gap-y-1.5",
									"rounded-md border border-border bg-muted/25 px-3 py-2",
								)}
							>
								{[
									["Added", preview.counts.added, "text-success"],
									["Skipped", preview.counts.skipped, "text-muted-foreground"],
									["Failed", preview.counts.failed, "text-destructive"],
									["Warnings", preview.counts.warnings, "text-warning"],
								].map(([label, count, color]) => (
									<div
										key={label}
										className="inline-flex items-baseline gap-1.5 whitespace-nowrap"
									>
										<dt className="text-xs text-muted-foreground">{label}</dt>
										<dd
											className={cn(
												"text-sm font-semibold tabular-nums",
												color,
											)}
										>
											{count}
										</dd>
									</div>
								))}
							</dl>
							{preview.items.length > 0 ? (
								<ul className="divide-y divide-border overflow-hidden rounded-md border border-border">
									{preview.items.map((item) => (
										<li
											key={item.source_index}
											className="space-y-1 px-3 py-2.5 text-xs"
										>
											<div className="flex flex-wrap items-center gap-2">
												<span
													className={cn(
														"rounded-full px-2 py-0.5 text-[11px] font-medium",
														statusClass(item.status),
													)}
												>
													{statusLabel(item.status)}
												</span>
												<span className="font-mono text-muted-foreground">
													#{item.source_index}
												</span>
												{item.scheme ? (
													<span className="font-mono">{item.scheme}</span>
												) : null}
												{item.name ? (
													<span
														className="max-w-full min-w-0 break-words font-medium"
														title={item.name}
													>
														{item.name}
													</span>
												) : null}
											</div>
											{item.reason ? (
												<div className="break-words text-muted-foreground">
													{item.reason}
												</div>
											) : null}
											{item.warnings.length > 0 ? (
												<div className="break-words text-warning-foreground">
													{item.warnings.join(" · ")}
												</div>
											) : null}
										</li>
									))}
								</ul>
							) : (
								<div
									className={cn(
										"rounded-md border border-dashed border-border px-3 py-4",
										"text-xs text-muted-foreground",
									)}
								>
									No candidate nodes were returned.
								</div>
							)}
						</section>
					) : null}
				</div>

				<SheetFooter
					className={cn(
						"shrink-0 border-t border-border px-5 pt-4",
						"pb-[calc(1rem+env(safe-area-inset-bottom))]",
						"sm:px-6 sm:py-4",
					)}
				>
					<Button variant="outline" onClick={close}>
						Cancel
					</Button>
					<Button
						variant="secondary"
						loading={isLoading}
						disabled={readOnly || isLoading}
						iconLeft={<Icon name="tabler:eye" />}
						onClick={() => void handlePreview()}
					>
						Preview
					</Button>
					<Button
						disabled={!canAdd}
						iconLeft={<Icon name="tabler:plus" />}
						onClick={handleAdd}
					>
						Add valid nodes
					</Button>
				</SheetFooter>
			</SheetContent>
		</Sheet>
	);
}

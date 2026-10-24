import { useCallback, useEffect, useRef, useState } from "react";

import {
	type AdminMihomoImportPreview,
	previewAdminUserMihomoImport,
} from "../api/adminUsers";
import { cn } from "../lib/utils";
import { Button } from "./Button";
import { Icon } from "./Icon";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
} from "./ui/dialog";

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
}: MihomoSubscriptionImportDialogProps) {
	const [source, setSource] = useState("");
	const [preview, setPreview] = useState<AdminMihomoImportPreview | null>(null);
	const [previewBaseline, setPreviewBaseline] = useState<string | null>(null);
	const [isLoading, setIsLoading] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const abortRef = useRef<AbortController | null>(null);
	const requestIdRef = useRef(0);

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
			setPreviewBaseline(existingExtraProxiesYaml);
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
		previewRequest,
		source,
		userId,
	]);

	const isStale =
		preview !== null &&
		previewBaseline !== null &&
		previewBaseline !== existingExtraProxiesYaml;
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
		<Dialog open={open} onOpenChange={(nextOpen) => !nextOpen && close()}>
			<DialogContent
				className={cn(
					"h-[min(46rem,calc(100dvh-2rem))] max-h-[min(46rem,calc(100dvh-2rem))]",
					"grid-rows-[auto_minmax(0,1fr)_auto] gap-0 overflow-hidden p-0 sm:max-w-2xl",
				)}
			>
				<DialogHeader className="shrink-0 border-b border-border px-5 py-4 sm:px-6">
					<DialogTitle className="flex items-center gap-2 text-base">
						<Icon
							name="tabler:download"
							className="size-5 text-primary"
							ariaLabel=""
						/>
						Import Mihomo nodes
					</DialogTitle>
					<DialogDescription>
						Paste a URL, node URI, Base64 text, or Mihomo YAML to preview valid
						nodes.
					</DialogDescription>
				</DialogHeader>

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
							<div className="flex flex-wrap items-center justify-between gap-2">
								<div className="flex items-center gap-2 text-sm font-medium">
									<span>Preview report</span>
									<span className="rounded-full bg-primary/10 px-2 py-0.5 text-[11px] text-primary">
										{SOURCE_LABELS[preview.detected_source]}
									</span>
								</div>
								<span className="text-xs text-muted-foreground">
									{preview.counts.warnings} warning
									{preview.counts.warnings === 1 ? "" : "s"}
								</span>
							</div>
							<div className="grid grid-cols-3 gap-2 sm:grid-cols-4">
								{[
									["Added", preview.counts.added, "text-success"],
									["Skipped", preview.counts.skipped, "text-muted-foreground"],
									["Failed", preview.counts.failed, "text-destructive"],
									["Warnings", preview.counts.warnings, "text-warning"],
								].map(([label, count, color]) => (
									<div
										key={label}
										className="rounded-md border border-border bg-muted/25 px-3 py-2"
									>
										<div className="text-[11px] text-muted-foreground">
											{label}
										</div>
										<div
											className={cn(
												"mt-1 text-lg font-semibold tabular-nums",
												color,
											)}
										>
											{count}
										</div>
									</div>
								))}
							</div>
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
													<span className="min-w-0 truncate font-medium">
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

				<DialogFooter className="shrink-0 border-t border-border px-5 py-4 sm:px-6">
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
				</DialogFooter>
			</DialogContent>
		</Dialog>
	);
}

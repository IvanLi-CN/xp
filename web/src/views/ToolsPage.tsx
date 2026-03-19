import { type FormEvent, useState } from "react";

import {
	type AdminMihomoRedactSourceKind,
	type AdminMihomoRedactionLevel,
	type AdminMihomoSourceFormat,
	redactAdminMihomo,
} from "../api/adminTools";
import { isBackendApiError } from "../api/backendError";
import { Button } from "../components/Button";
import { CopyButton } from "../components/CopyButton";
import { MihomoSplitEditor } from "../components/MihomoSplitEditor";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { useUiPrefs } from "../components/UiPrefs";
import { YamlCodeEditor } from "../components/YamlCodeEditor";
import { readAdminToken } from "../components/auth";
import {
	inputClass as inputControlClass,
	selectClass as selectControlClass,
} from "../components/ui-helpers";
import { Input } from "../components/ui/input";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "../components/ui/select";

const SOURCE_KIND_OPTIONS: Array<{
	value: AdminMihomoRedactSourceKind;
	label: string;
}> = [
	{ value: "text", label: "text" },
	{ value: "url", label: "url" },
];

const SOURCE_FORMAT_OPTIONS: Array<{
	value: AdminMihomoSourceFormat;
	label: string;
}> = [
	{ value: "auto", label: "auto" },
	{ value: "raw", label: "raw" },
	{ value: "base64", label: "base64" },
	{ value: "yaml", label: "yaml" },
];

const REDACTION_LEVEL_OPTIONS: Array<{
	value: AdminMihomoRedactionLevel;
	label: string;
}> = [
	{ value: "minimal", label: "minimal" },
	{ value: "credentials", label: "credentials" },
	{
		value: "credentials_and_address",
		label: "credentials + address",
	},
];

function formatErrorMessage(error: unknown): string {
	if (isBackendApiError(error)) {
		const code = error.code ? ` ${error.code}` : "";
		return `${error.status}${code}: ${error.message}`;
	}
	return error instanceof Error ? error.message : String(error);
}

export function ToolsPage() {
	const [adminToken] = useState(() => readAdminToken());
	const prefs = useUiPrefs();
	const inputClassName = inputControlClass(prefs.density);
	const selectClassName = selectControlClass(prefs.density);
	const [sourceKind, setSourceKind] =
		useState<AdminMihomoRedactSourceKind>("text");
	const [sourceFormat, setSourceFormat] =
		useState<AdminMihomoSourceFormat>("auto");
	const [level, setLevel] = useState<AdminMihomoRedactionLevel>("credentials");
	const [source, setSource] = useState("");
	const [redactedText, setRedactedText] = useState("");
	const [error, setError] = useState<string | null>(null);
	const [isSubmitting, setIsSubmitting] = useState(false);

	if (adminToken.length === 0) {
		return (
			<PageState
				variant="empty"
				title="Admin token required"
				description="Open Dashboard and configure an admin token before using tools."
			/>
		);
	}

	const sourceLabel = sourceKind === "url" ? "Source URL" : "Source text";
	const sourceHint =
		sourceKind === "url"
			? "Only public http/https URLs are allowed. Paste local or private sources as text instead."
			: "Paste raw / base64 subscription text or Mihomo YAML here. Execution runs only when you click the button.";
	const sourcePlaceholder =
		sourceKind === "url"
			? "https://example.com/subscription"
			: "vless://...\nss://...\n# or Mihomo YAML";

	async function handleSubmit(event: FormEvent<HTMLFormElement>) {
		event.preventDefault();
		if (source.trim().length === 0) {
			setError("Source is required.");
			setRedactedText("");
			return;
		}

		setIsSubmitting(true);
		setError(null);
		try {
			const response = await redactAdminMihomo(adminToken, {
				source_kind: sourceKind,
				source,
				level,
				source_format: sourceFormat,
			});
			setRedactedText(response.redacted_text);
		} catch (nextError) {
			setRedactedText("");
			setError(formatErrorMessage(nextError));
		} finally {
			setIsSubmitting(false);
		}
	}

	return (
		<div className="space-y-6">
			<PageHeader
				title="Tools"
				description="Safe admin-side helpers for one-off diagnostics and shareable outputs."
			/>

			<section className="xp-card">
				<div className="xp-card-body space-y-5">
					<div>
						<h2 className="text-base font-semibold">Mihomo redact</h2>
						<p className="text-sm text-muted-foreground">
							Redact Mihomo subscriptions or configs without mutating the
							original source.
						</p>
					</div>

					<form className="space-y-5" onSubmit={handleSubmit}>
						<div className="grid gap-4 md:grid-cols-3">
							<div className="xp-field-stack gap-2">
								<span className="text-sm font-medium">Source kind</span>
								<Select
									value={sourceKind}
									onValueChange={(value) => {
										setSourceKind(value as AdminMihomoRedactSourceKind);
										setError(null);
									}}
								>
									<SelectTrigger
										aria-label="Source kind"
										className={selectClassName}
									>
										<SelectValue />
									</SelectTrigger>
									<SelectContent>
										{SOURCE_KIND_OPTIONS.map((option) => (
											<SelectItem key={option.value} value={option.value}>
												{option.label}
											</SelectItem>
										))}
									</SelectContent>
								</Select>
							</div>

							<div className="xp-field-stack gap-2">
								<span className="text-sm font-medium">Source format</span>
								<Select
									value={sourceFormat}
									onValueChange={(value) =>
										setSourceFormat(value as AdminMihomoSourceFormat)
									}
								>
									<SelectTrigger
										aria-label="Source format"
										className={selectClassName}
									>
										<SelectValue />
									</SelectTrigger>
									<SelectContent>
										{SOURCE_FORMAT_OPTIONS.map((option) => (
											<SelectItem key={option.value} value={option.value}>
												{option.label}
											</SelectItem>
										))}
									</SelectContent>
								</Select>
							</div>

							<div className="xp-field-stack gap-2">
								<span className="text-sm font-medium">Redaction level</span>
								<Select
									value={level}
									onValueChange={(value) =>
										setLevel(value as AdminMihomoRedactionLevel)
									}
								>
									<SelectTrigger
										aria-label="Redaction level"
										className={selectClassName}
									>
										<SelectValue />
									</SelectTrigger>
									<SelectContent>
										{REDACTION_LEVEL_OPTIONS.map((option) => (
											<SelectItem key={option.value} value={option.value}>
												{option.label}
											</SelectItem>
										))}
									</SelectContent>
								</Select>
							</div>
						</div>

						{error ? (
							<div className="rounded-xl border border-destructive/30 bg-destructive/10 px-4 py-2 text-sm text-destructive">
								{error}
							</div>
						) : null}

						{sourceKind === "url" ? (
							<div className="grid gap-5 xl:grid-cols-2">
								<div className="space-y-3">
									<div className="space-y-1">
										<h3 className="text-sm font-semibold">{sourceLabel}</h3>
										<p className="text-xs text-muted-foreground">
											{sourceHint}
										</p>
									</div>
									<div className="rounded-2xl border border-border/70 bg-muted/20 p-4">
										<Input
											id="mihomo-source"
											aria-label={sourceLabel}
											className={inputClassName}
											value={source}
											placeholder={sourcePlaceholder}
											onChange={(event) => setSource(event.target.value)}
										/>
									</div>
								</div>

								<div className="space-y-3">
									<div className="space-y-1">
										<h3 className="text-sm font-semibold">Preview</h3>
										<p className="text-xs text-muted-foreground">
											Read-only output preserves line breaks so you can verify
											the redaction before sharing it.
										</p>
									</div>

									<YamlCodeEditor
										label="Redacted result"
										value={redactedText}
										onChange={() => {}}
										placeholder="Run the tool to render a sanitized preview here."
										minRows={18}
										readOnly
										hideLabel
										helperText="Read-only preview · line numbers · fold · Ctrl/Cmd+F"
									/>
								</div>
							</div>
						) : (
							<MihomoSplitEditor
								originalLabel={sourceLabel}
								originalDescription={sourceHint}
								originalValue={source}
								onOriginalChange={setSource}
								originalPlaceholder={sourcePlaceholder}
								modifiedLabel="Redacted result"
								modifiedDescription="Read-only output preserves line breaks so you can verify the redaction before sharing it."
								modifiedValue={redactedText}
								modifiedPlaceholder="Run the tool to render a sanitized preview here."
								minRows={18}
							/>
						)}

						<div className="flex flex-wrap items-end justify-between gap-3">
							<div className="flex flex-wrap items-center gap-3">
								<Button
									type="submit"
									loading={isSubmitting}
									disabled={source.trim().length === 0}
								>
									Run redact
								</Button>
								<span className="text-xs text-muted-foreground">
									Output is always preview-only and never writes back to the
									source.
								</span>
							</div>
							{redactedText ? (
								<CopyButton
									text={redactedText}
									label="Copy result"
									ariaLabel="Copy redacted result"
								/>
							) : null}
						</div>
					</form>
				</div>
			</section>
		</div>
	);
}

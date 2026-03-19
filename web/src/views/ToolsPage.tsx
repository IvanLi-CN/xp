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
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { useUiPrefs } from "../components/UiPrefs";
import { readAdminToken } from "../components/auth";
import {
	inputClass as inputControlClass,
	selectClass as selectControlClass,
	textareaClass,
} from "../components/ui-helpers";
import { Input } from "../components/ui/input";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "../components/ui/select";
import { Textarea } from "../components/ui/textarea";

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

			<div className="grid gap-6 xl:grid-cols-[minmax(0,28rem)_minmax(0,1fr)]">
				<section className="xp-card">
					<div className="xp-card-body space-y-4">
						<div>
							<h2 className="text-base font-semibold">Mihomo redact</h2>
							<p className="text-sm text-muted-foreground">
								Redact Mihomo subscriptions or configs without mutating the
								original source.
							</p>
						</div>

						<form className="space-y-4" onSubmit={handleSubmit}>
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

							<div className="xp-field-stack gap-2">
								<label className="text-sm font-medium" htmlFor="mihomo-source">
									{sourceLabel}
								</label>
								{sourceKind === "url" ? (
									<Input
										id="mihomo-source"
										className={inputClassName}
										value={source}
										placeholder={sourcePlaceholder}
										onChange={(event) => setSource(event.target.value)}
									/>
								) : (
									<Textarea
										id="mihomo-source"
										className={textareaClass(
											"min-h-64 font-mono text-sm leading-6",
										)}
										value={source}
										placeholder={sourcePlaceholder}
										spellCheck={false}
										onChange={(event) => setSource(event.target.value)}
									/>
								)}
								<p className="text-xs text-muted-foreground">{sourceHint}</p>
							</div>

							{error ? (
								<div className="rounded-xl border border-destructive/30 bg-destructive/10 px-4 py-2 text-sm text-destructive">
									{error}
								</div>
							) : null}

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
						</form>
					</div>
				</section>

				<section className="xp-card">
					<div className="xp-card-body space-y-4">
						<div className="flex flex-wrap items-center justify-between gap-3">
							<div>
								<h2 className="text-base font-semibold">Preview</h2>
								<p className="text-sm text-muted-foreground">
									Read-only output preserves line breaks so you can verify the
									redaction before sharing it.
								</p>
							</div>
							{redactedText ? (
								<CopyButton
									text={redactedText}
									label="Copy result"
									ariaLabel="Copy redacted result"
								/>
							) : null}
						</div>

						{redactedText ? (
							<Textarea
								readOnly
								aria-label="Redacted result"
								className={textareaClass(
									"min-h-[28rem] font-mono text-sm leading-6",
								)}
								value={redactedText}
								spellCheck={false}
							/>
						) : (
							<div className="rounded-2xl border border-dashed border-border/70 bg-muted/30 px-4 py-8 text-sm text-muted-foreground">
								Run the tool to render a sanitized preview here.
							</div>
						)}
					</div>
				</section>
			</div>
		</div>
	);
}

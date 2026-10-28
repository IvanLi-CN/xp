import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, waitFor, within } from "@storybook/test";
import { useRef, useState } from "react";

import type {
	AdminMihomoImportPreview,
	AdminUserMihomoProfile,
} from "../api/adminUsers";
import type { MihomoSaveFeedback } from "../utils/mihomoSaveOutcome";
import { MihomoProfileEditor } from "./MihomoProfileEditor";
import type { MihomoImportPreviewRequestFn } from "./MihomoSubscriptionImportDialog";

const INITIAL_PROFILE: AdminUserMihomoProfile = {
	mixin_yaml: "port: 0\nproxy-groups:\n  - name: Auto\n    type: url-test\n",
	extra_proxies_yaml:
		"- name: custom-ss\n  type: ss\n  server: edge.example.com\n",
	extra_proxy_providers_yaml:
		"ProviderA:\n  type: http\n  url: https://example.com/provider.yaml\n",
};

type EditorStoryProps = {
	initialProfile?: AdminUserMihomoProfile;
	readOnly?: boolean;
	startWithError?: boolean;
	startSaving?: boolean;
	holdSave?: boolean;
	startWithUnknown?: boolean;
	importAvailable?: boolean;
};

const IMPORT_PREVIEW: AdminMihomoImportPreview = {
	detected_source: "raw",
	added_proxies_yaml: [
		"- name: imported-edge\n",
		"  type: vless\n",
		"  server: edge.example.com\n",
		"  port: 443\n",
		"  uuid: 00000000-0000-0000-0000-000000000001\n",
	].join(""),
	counts: { added: 1, skipped: 0, failed: 0, warnings: 0 },
	items: [
		{
			source_index: 1,
			status: "added",
			name: "imported-edge",
			scheme: "vless",
			warnings: [],
		},
	],
};

function EditorStory({
	initialProfile = INITIAL_PROFILE,
	readOnly = false,
	startWithError = false,
	startSaving = false,
	holdSave = false,
	startWithUnknown = false,
	importAvailable = false,
}: EditorStoryProps) {
	const initialDraft =
		startWithError || holdSave || startWithUnknown
			? {
					...initialProfile,
					mixin_yaml: `${initialProfile.mixin_yaml}# retry me\n`,
					extra_proxies_yaml: `${initialProfile.extra_proxies_yaml}# keep me\n`,
					extra_proxy_providers_yaml: `${initialProfile.extra_proxy_providers_yaml}# keep me too\n`,
				}
			: initialProfile;
	const [draft, setDraft] = useState(initialDraft);
	const [savedProfile, setSavedProfile] = useState(initialProfile);
	const [isSaving, setIsSaving] = useState(startSaving);
	const [isVerifying, setIsVerifying] = useState(false);
	const [saveAttempts, setSaveAttempts] = useState(0);
	const [error, setError] = useState<string | null>(null);
	const [saveFeedback, setSaveFeedback] = useState<MihomoSaveFeedback | null>(
		startWithUnknown
			? {
					kind: "outcome_unknown",
					verification: "different",
					diagnostics: {
						code: "raft_write_outcome_unknown",
						dispatchState: "dispatched_no_verified_response",
						supportId: "01JDEMO-SUPPORT",
						targetNodeId: "node-demo",
					},
				}
			: null,
	);
	const [visualEvidenceTarget, setVisualEvidenceTarget] =
		useState<HTMLDivElement | null>(null);
	const pendingSaveResolve = useRef<(() => void) | null>(null);

	const save = async () => {
		setIsSaving(true);
		setError(null);
		if (holdSave) {
			await new Promise<void>((resolve) => {
				pendingSaveResolve.current = resolve;
			});
		}
		setIsSaving(false);
		if (startWithError && saveAttempts === 0) {
			setSaveAttempts(1);
			setError("The previous save failed. Retry when the service is ready.");
			return false;
		}
		if (!holdSave) {
			setSavedProfile(draft);
		}
		setSaveFeedback(null);
		return true;
	};
	const verify = async () => {
		setIsVerifying(true);
		await Promise.resolve();
		setSavedProfile(draft);
		setSaveFeedback({
			kind: "outcome_unknown",
			verification: "matches",
			diagnostics: {
				code: "raft_write_outcome_unknown",
				dispatchState: "dispatched_no_verified_response",
			},
		});
		setIsVerifying(false);
		return true;
	};

	return (
		<div
			className="min-h-[34rem] w-full bg-background p-12 text-foreground"
			data-visual-evidence-surface="mihomo-profile-editor"
		>
			<div
				ref={setVisualEvidenceTarget}
				data-visual-evidence-target="mihomo-profile-editor"
			>
				<MihomoProfileEditor
					userName="Mira Sato"
					userId="story-user"
					profile={savedProfile}
					draft={draft}
					dirty={Object.keys(savedProfile).some(
						(key) =>
							draft[key as keyof AdminUserMihomoProfile] !==
							savedProfile[key as keyof AdminUserMihomoProfile],
					)}
					isSaving={isSaving}
					isVerifying={isVerifying}
					readOnly={readOnly}
					error={error}
					saveFeedback={saveFeedback}
					isLoaded
					importAvailable={importAvailable}
					previewImport={
						(async () => IMPORT_PREVIEW) satisfies MihomoImportPreviewRequestFn
					}
					portalContainer={visualEvidenceTarget}
					onChange={(documentId, value) => {
						setDraft((current) => {
							switch (documentId) {
								case "mixin_yaml":
									return { ...current, mixin_yaml: value };
								case "extra_proxies_yaml":
									return { ...current, extra_proxies_yaml: value };
								case "extra_proxy_providers_yaml":
									return {
										...current,
										extra_proxy_providers_yaml: value,
									};
							}
						});
						setError(null);
					}}
					onSave={save}
					onVerify={verify}
				/>
				{holdSave ? (
					<button
						type="button"
						className="sr-only"
						aria-label="Resolve pending save"
						onClick={() => pendingSaveResolve.current?.()}
					>
						Resolve pending save
					</button>
				) : null}
			</div>
		</div>
	);
}

const meta = {
	title: "Components/MihomoProfileEditor",
	component: EditorStory,
	tags: ["autodocs", "coverage-ui", "mihomo-workspace"],
	parameters: {
		layout: "fullscreen",
		docs: {
			description: {
				component:
					"Shared Mihomo profile editor. Expanded mode keeps all three CodeMirror documents mounted.",
			},
		},
	},
	args: {},
} satisfies Meta<typeof EditorStory>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Inline: Story = {};

async function openImportPreview(canvasElement: HTMLElement) {
	const canvas = within(canvasElement);
	await userEvent.click(canvas.getByRole("button", { name: "Import nodes" }));
	const dialog = await within(document.body).findByRole("dialog");
	await userEvent.type(
		within(dialog).getByLabelText("Subscription or node content"),
		"vless://00000000-0000-0000-0000-000000000001@edge.example.com:443#imported-edge",
	);
	await userEvent.click(
		within(dialog).getByRole("button", { name: "Preview" }),
	);
	await expect(dialog).toHaveTextContent("Preview report");
	return dialog;
}

export const ImportPreview: Story = {
	args: { importAvailable: true },
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const dialog = await openImportPreview(canvasElement);
		await expect(dialog).toHaveTextContent("imported-edge");
		await userEvent.click(
			within(dialog).getByRole("button", { name: "Add valid nodes" }),
		);
		await expect(
			within(document.body).queryByRole("dialog"),
		).not.toBeInTheDocument();
		await expect(canvasElement).toHaveTextContent(
			"Unsaved configuration changes",
		);
		await userEvent.click(
			canvas.getByRole("button", { name: "Save configuration" }),
		);
		await waitFor(() =>
			expect(canvasElement).toHaveTextContent("Saved configuration"),
		);
	},
};

export const ImportPreviewReport: Story = {
	args: { importAvailable: true },
	play: async ({ canvasElement }) => {
		await openImportPreview(canvasElement);
	},
};

export const ImportPreviewMobile: Story = {
	args: { importAvailable: true },
	parameters: {
		viewport: {
			defaultViewport: "mihomoMobile393",
			viewports: {
				mihomoMobile393: {
					name: "Mihomo mobile (393x852)",
					styles: { width: "393px", height: "852px" },
					type: "mobile",
				},
			},
		},
	},
	play: async ({ canvasElement }) => {
		const dialog = await openImportPreview(canvasElement);
		await expect(dialog).toHaveClass("bottom-0", "rounded-t-2xl");
		await userEvent.click(
			within(dialog).getByRole("button", { name: "Add valid nodes" }),
		);
		await expect(
			within(document.body).queryByRole("dialog"),
		).not.toBeInTheDocument();
	},
};

export const ImportPreviewReportMobile: Story = {
	args: { importAvailable: true },
	parameters: ImportPreviewMobile.parameters,
	play: async ({ canvasElement }) => {
		const dialog = await openImportPreview(canvasElement);
		await expect(dialog).toHaveClass("bottom-0", "rounded-t-2xl");
	},
};

export const Expanded: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			canvas.getByRole("button", { name: "Expand editor" }),
		);
		const dialog = await within(document.body).findByRole("dialog");
		await expect(dialog).toHaveTextContent("mixin_yaml");
		await expect(
			within(dialog).getAllByRole("button", { name: /yaml/ }),
		).toHaveLength(3);
		await userEvent.click(
			within(dialog).getByRole("button", { name: /extra_proxies_yaml/ }),
		);
		await expect(dialog).toHaveTextContent("extra_proxies_yaml");
	},
};

export const LightTheme: Story = {
	globals: { theme: "light" },
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			canvas.getByRole("button", { name: "Expand editor" }),
		);
		const dialog = await within(document.body).findByRole("dialog");
		await expect(dialog).toBeVisible();
		await expect(document.documentElement).toHaveAttribute(
			"data-theme",
			"xp-light",
		);
		await expect(
			within(dialog).getByTestId("mihomo-workspace-surface"),
		).toBeVisible();
	},
};

export const ReadOnly: Story = {
	args: { readOnly: true },
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			canvas.getByRole("button", { name: "Expand editor" }),
		);
		const dialog = await within(document.body).findByRole("dialog");
		await expect(
			within(dialog).getByRole("button", { name: "Save configuration" }),
		).toBeDisabled();
		await waitFor(() =>
			expect(dialog.querySelectorAll<HTMLElement>(".cm-content")).toHaveLength(
				3,
			),
		);
		const editors = Array.from(
			dialog.querySelectorAll<HTMLElement>(".cm-content"),
		);
		for (const editor of editors) {
			await expect(editor).toHaveAttribute("contenteditable", "false");
		}
	},
};

export const SaveError: Story = {
	args: { startWithError: true },
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			canvas.getByRole("button", { name: "Expand editor" }),
		);
		const dialog = await within(document.body).findByRole("dialog");
		const saveButton = within(dialog).getByRole("button", {
			name: "Save configuration",
		});
		expect(saveButton).toBeEnabled();
		await userEvent.click(saveButton);
		await expect(within(dialog).getByRole("alert")).toHaveTextContent(
			"The previous save failed",
		);
		await userEvent.click(
			within(dialog).getByRole("button", { name: /extra_proxies_yaml/ }),
		);
		await expect(dialog).toHaveTextContent("keep me");
		await userEvent.click(
			within(dialog).getByRole("button", {
				name: /extra_proxy_providers_yaml/,
			}),
		);
		await expect(dialog).toHaveTextContent("keep me too");
		await userEvent.click(saveButton);
		await expect(within(dialog).queryByRole("alert")).not.toBeInTheDocument();
	},
};

export const OutcomeUnknown: Story = {
	args: { startWithUnknown: true },
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByText("Save result is still unknown"),
		).toBeVisible();
		await expect(
			canvas.getByRole("button", { name: "Check again" }),
		).toBeEnabled();
		await userEvent.click(canvas.getByRole("button", { name: "Retry save" }));
		await expect(canvasElement).toHaveTextContent(
			"Click Retry save again to confirm",
		);
	},
};

export const Saving: Story = {
	args: { holdSave: true },
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const inlineSave = canvas.getByRole("button", {
			name: "Save configuration",
		});
		await userEvent.click(inlineSave);
		await userEvent.click(
			canvas.getByRole("button", { name: "Expand editor" }),
		);
		const dialog = await within(document.body).findByRole("dialog");
		await expect(
			within(dialog).getByRole("button", { name: "Save configuration" }),
		).toBeDisabled();
		for (const editor of Array.from(
			dialog.querySelectorAll<HTMLElement>(".cm-content"),
		)) {
			await expect(editor).toHaveAttribute("contenteditable", "false");
		}
		await userEvent.click(
			within(dialog).getByRole("button", { name: "Exit expanded editor" }),
		);
		await expect(
			canvas.getByRole("button", { name: "Resolve pending save" }),
		).toBeInTheDocument();
		await userEvent.click(
			canvas.getByRole("button", { name: "Resolve pending save" }),
		);
		await expect(inlineSave).toBeEnabled();
	},
};

export const MobileFiles: Story = {
	args: {},
	parameters: {
		viewport: {
			defaultViewport: "mihomoMobile393",
			viewports: {
				mihomoMobile393: {
					name: "Mihomo mobile (393x852)",
					styles: { width: "393px", height: "852px" },
					type: "mobile",
				},
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			canvas.getByRole("button", { name: "Expand editor" }),
		);
		const dialog = await within(document.body).findByRole("dialog");
		const filesButton = within(dialog).queryByRole("button", {
			name: "Files",
		});
		if (filesButton) {
			await userEvent.click(filesButton);
			await expect(
				within(document.body).getByRole("heading", { name: "Files" }),
			).toBeInTheDocument();
			await userEvent.click(
				within(document.body).getByRole("button", {
					name: /extra_proxies_yaml/,
				}),
			);
			await expect(filesButton).toHaveFocus();
		} else {
			await expect(dialog.querySelector("aside")).not.toBeNull();
		}
	},
};

import type { Meta, StoryObj } from "@storybook/react";
import { expect, userEvent, within } from "@storybook/test";
import { useState } from "react";

import type { AdminUserMihomoProfile } from "../api/adminUsers";
import { MihomoProfileEditor } from "./MihomoProfileEditor";

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
};

function EditorStory({
	initialProfile = INITIAL_PROFILE,
	readOnly = false,
	startWithError = false,
	startSaving = false,
}: EditorStoryProps) {
	const initialDraft = startWithError
		? {
				...initialProfile,
				mixin_yaml: `${initialProfile.mixin_yaml}# retry me\n`,
				extra_proxies_yaml: `${initialProfile.extra_proxies_yaml}# keep me\n`,
				extra_proxy_providers_yaml: `${initialProfile.extra_proxy_providers_yaml}# keep me too\n`,
			}
		: initialProfile;
	const [draft, setDraft] = useState(initialDraft);
	const [isSaving, setIsSaving] = useState(startSaving);
	const [saveAttempts, setSaveAttempts] = useState(0);
	const [error, setError] = useState<string | null>(null);

	const save = async () => {
		setIsSaving(true);
		setError(null);
		setIsSaving(false);
		if (startWithError && saveAttempts === 0) {
			setSaveAttempts(1);
			setError("The previous save failed. Retry when the service is ready.");
			return false;
		}
		return true;
	};

	return (
		<div
			className="min-h-[34rem] w-full bg-background p-6 text-foreground"
			data-visual-evidence-surface="mihomo-profile-editor"
		>
			<div data-visual-evidence-target="mihomo-profile-editor">
				<MihomoProfileEditor
					userName="Mira Sato"
					userId="story-user"
					profile={initialProfile}
					draft={draft}
					dirty={Object.keys(initialProfile).some(
						(key) =>
							draft[key as keyof AdminUserMihomoProfile] !==
							initialProfile[key as keyof AdminUserMihomoProfile],
					)}
					isSaving={isSaving}
					readOnly={readOnly}
					error={error}
					isLoaded
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
				/>
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
		const editors = Array.from(
			dialog.querySelectorAll<HTMLElement>(".cm-content"),
		);
		await expect(editors).toHaveLength(3);
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

export const Saving: Story = {
	args: { startSaving: true },
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
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

import type { Meta, StoryObj } from "@storybook/react";
import { useEffect, useState } from "react";

import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";

import { YamlCodeEditor } from "./YamlCodeEditor";

type YamlCodeEditorStoryProps = {
	label: string;
	value: string;
	placeholder?: string;
	minRows?: number;
};

function YamlCodeEditorStory({
	label,
	value,
	placeholder,
	minRows,
}: YamlCodeEditorStoryProps) {
	const [draft, setDraft] = useState(value);

	useEffect(() => {
		setDraft(value);
	}, [value]);

	return (
		<Card className="w-[min(100%,760px)]">
			<CardHeader>
				<CardTitle>{label}</CardTitle>
				<CardDescription>
					Theme follows the Storybook `theme` toolbar via `UiPrefs`; density is
					intentionally neutral so editor readability stays stable.
				</CardDescription>
			</CardHeader>
			<CardContent className="space-y-3">
				<YamlCodeEditor
					label={label}
					value={draft}
					onChange={setDraft}
					placeholder={placeholder}
					minRows={minRows}
				/>
				<div className="rounded-xl border border-border/70 bg-muted/30 p-3">
					<div className="text-xs uppercase tracking-[0.12em] text-muted-foreground">
						Current value
					</div>
					<pre className="mt-2 max-h-48 overflow-auto whitespace-pre-wrap font-mono text-xs text-foreground">
						{draft || "# empty"}
					</pre>
				</div>
			</CardContent>
		</Card>
	);
}

const SAMPLE_REALITY = `dest: example.com:443
server_names:
  - example.com
  - assets.example.com
fingerprint: chrome
`;

const meta = {
	title: "Components/YamlCodeEditor",
	component: YamlCodeEditorStory,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		docs: {
			description: {
				component:
					"YAML editor wrapper used for service config and reality-domain workflows. Stories cover the default editable state and the empty-template edge state; switch the Storybook theme toolbar to verify the CodeMirror light/dark palette swap.",
			},
		},
	},
	args: {
		label: "Reality config",
		value: SAMPLE_REALITY,
		minRows: 8,
	},
} satisfies Meta<typeof YamlCodeEditorStory>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const TallDocument: Story = {
	args: {
		label: "Service config",
		minRows: 12,
		value: `listen: 0.0.0.0:443
mode: rule
routes:
  - match: 0.0.0.0/0
    via: edge-main
  - match: 10.0.0.0/8
    via: internal
observability:
  metrics: true
  tracing: false
`,
	},
};

export const EmptyTemplate: Story = {
	args: {
		label: "Subscription template",
		value: "",
		placeholder: "routes:\n  - cidr: 10.0.0.0/8\n    via: internal",
		minRows: 6,
	},
};

import type { Meta, StoryObj } from "@storybook/react";
import { useEffect } from "react";

import { Badge } from "@/components/ui/badge";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import { cn } from "@/lib/utils";

import { Button } from "./Button";
import {
	UI_DENSITY_STORAGE_KEY,
	UI_THEME_STORAGE_KEY,
	type UiDensity,
	type UiThemePreference,
	useUiPrefs,
} from "./UiPrefs";

type UiPrefsStoryProps = {
	initialTheme: UiThemePreference;
	initialDensity: UiDensity;
};

function readStorage(key: string): string {
	try {
		return localStorage.getItem(key) ?? "-";
	} catch {
		return "-";
	}
}

function UiPrefsStory({ initialTheme, initialDensity }: UiPrefsStoryProps) {
	const { theme, resolvedTheme, density, setTheme, setDensity } = useUiPrefs();

	useEffect(() => {
		setTheme(initialTheme);
		setDensity(initialDensity);
	}, [initialDensity, initialTheme, setDensity, setTheme]);

	const html = document.documentElement;
	const dataTheme = html.getAttribute("data-theme") ?? "-";
	const dataDensity = html.getAttribute("data-density") ?? "-";
	const darkClass = html.classList.contains("dark") ? "present" : "absent";

	return (
		<Card className="w-[460px]">
			<CardHeader>
				<CardTitle>UI preferences</CardTitle>
				<CardDescription>
					Persists `xp_ui_theme` / `xp_ui_density` and applies the matching
					`data-theme`, `data-density`, and `dark` class on the document root.
				</CardDescription>
			</CardHeader>
			<CardContent className="space-y-4">
				<div className="flex flex-wrap gap-2">
					<Badge variant={resolvedTheme === "dark" ? "default" : "secondary"}>
						resolved: {resolvedTheme}
					</Badge>
					<Badge variant="outline">theme: {theme}</Badge>
					<Badge variant="outline">density: {density}</Badge>
				</div>

				<div className="space-y-2">
					<div className="text-xs uppercase tracking-[0.12em] text-muted-foreground">
						Theme
					</div>
					<div className="flex flex-wrap gap-2">
						{(["system", "light", "dark"] as const).map((option) => (
							<Button
								key={option}
								size="sm"
								variant={theme === option ? "primary" : "secondary"}
								onClick={() => setTheme(option)}
							>
								{option}
							</Button>
						))}
					</div>
				</div>

				<div className="space-y-2">
					<div className="text-xs uppercase tracking-[0.12em] text-muted-foreground">
						Density
					</div>
					<div className="flex flex-wrap gap-2">
						{(["comfortable", "compact"] as const).map((option) => (
							<Button
								key={option}
								size="sm"
								variant={density === option ? "primary" : "secondary"}
								onClick={() => setDensity(option)}
							>
								{option}
							</Button>
						))}
					</div>
				</div>

				<div
					className={cn(
						"rounded-2xl border border-border/70 bg-muted/30 transition-colors",
						density === "compact" ? "space-y-2 p-3 text-sm" : "space-y-3 p-4",
					)}
				>
					<div className="font-medium">Shell preview</div>
					<p className="text-muted-foreground">
						Buttons, dialog shells, tables, and form controls reuse these
						preferences through app wrappers.
					</p>
				</div>

				<dl className="grid grid-cols-1 gap-2 text-sm sm:grid-cols-2">
					<div className="rounded-xl border border-border/60 bg-background px-3 py-2">
						<dt className="text-xs text-muted-foreground">
							localStorage theme
						</dt>
						<dd className="font-mono">{readStorage(UI_THEME_STORAGE_KEY)}</dd>
					</div>
					<div className="rounded-xl border border-border/60 bg-background px-3 py-2">
						<dt className="text-xs text-muted-foreground">
							localStorage density
						</dt>
						<dd className="font-mono">{readStorage(UI_DENSITY_STORAGE_KEY)}</dd>
					</div>
					<div className="rounded-xl border border-border/60 bg-background px-3 py-2">
						<dt className="text-xs text-muted-foreground">
							document data-theme
						</dt>
						<dd className="font-mono">{dataTheme}</dd>
					</div>
					<div className="rounded-xl border border-border/60 bg-background px-3 py-2">
						<dt className="text-xs text-muted-foreground">
							document density / dark
						</dt>
						<dd className="font-mono">
							{dataDensity} / {darkClass}
						</dd>
					</div>
				</dl>
			</CardContent>
		</Card>
	);
}

const meta = {
	title: "Components/UiPrefs",
	component: UiPrefsStory,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"Visual harness for the `UiPrefsProvider` contract. Use it to verify storage persistence, `dark` class toggling, and density-driven shell spacing. The Storybook toolbar still sets the initial globals before this story applies its own starting state.",
			},
		},
	},
	args: {
		initialTheme: "dark",
		initialDensity: "comfortable",
	},
} satisfies Meta<typeof UiPrefsStory>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const LightCompact: Story = {
	args: {
		initialTheme: "light",
		initialDensity: "compact",
	},
};

export const SystemTheme: Story = {
	args: {
		initialTheme: "system",
		initialDensity: "comfortable",
	},
};

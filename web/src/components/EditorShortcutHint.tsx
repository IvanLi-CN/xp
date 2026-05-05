import { useMemo } from "react";

import { Icon } from "./Icon";

type EditorShortcutPlatform = "auto" | "mac" | "windows";

function useIsMacPlatform(): boolean {
	return useMemo(() => {
		if (typeof navigator === "undefined") return false;
		const ua = `${navigator.userAgent} ${navigator.platform}`.toLowerCase();
		return ua.includes("mac") || ua.includes("iphone") || ua.includes("ipad");
	}, []);
}

export function useEditorShortcutItems(
	platform: EditorShortcutPlatform = "auto",
): Array<{
	label: string;
	combos: string[][];
}> {
	const autoIsMac = useIsMacPlatform();
	const isMac = platform === "auto" ? autoIsMac : platform === "mac";

	return useMemo(
		() => [
			{
				label: "Search",
				combos: [isMac ? ["⌘", "F"] : ["Ctrl", "F"]],
			},
			{
				label: "Fold",
				combos: [
					isMac ? ["⌘", "⌥", "["] : ["Ctrl", "Shift", "["],
					isMac ? ["⌃", "⌥", "["] : ["Ctrl", "Alt", "["],
				],
			},
			{
				label: "Unfold",
				combos: [
					isMac ? ["⌘", "⌥", "]"] : ["Ctrl", "Shift", "]"],
					isMac ? ["⌃", "⌥", "]"] : ["Ctrl", "Alt", "]"],
				],
			},
		],
		[isMac],
	);
}

type EditorShortcutHintProps = {
	platform?: EditorShortcutPlatform;
};

export function EditorShortcutHint({
	platform = "auto",
}: EditorShortcutHintProps) {
	const shortcuts = useEditorShortcutItems(platform);

	return (
		<div className="flex flex-wrap items-center gap-x-4 gap-y-2 text-[11px] leading-5 text-muted-foreground">
			<div className="flex shrink-0 items-center gap-1.5">
				<Icon name="tabler:keyboard" size={14} />
				<span className="font-medium text-foreground/80">Shortcuts</span>
			</div>
			<div className="flex flex-wrap items-center gap-x-4 gap-y-2">
				{shortcuts.map((shortcut) => (
					<div
						key={shortcut.label}
						className="inline-flex items-center gap-2 whitespace-nowrap"
					>
						<span>{shortcut.label}</span>
						<div className="inline-flex items-center gap-2">
							{shortcut.combos.map((combo, comboIndex) => (
								<div
									key={`${shortcut.label}-${String(comboIndex)}`}
									className="inline-flex items-center gap-1"
								>
									{comboIndex > 0 ? (
										<span className="text-muted-foreground/60">/</span>
									) : null}
									{combo.map((key, keyIndex) => (
										<kbd
											key={`${shortcut.label}-${String(comboIndex)}-${key}-${String(keyIndex)}`}
											className="inline-flex h-6 min-w-7 items-center justify-center rounded border border-border bg-muted px-1.5 font-mono text-[10px] font-semibold tracking-tight text-foreground shadow-xs"
										>
											{key}
										</kbd>
									))}
								</div>
							))}
						</div>
					</div>
				))}
			</div>
		</div>
	);
}

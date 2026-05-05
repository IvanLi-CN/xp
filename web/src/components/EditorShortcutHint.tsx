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
	keys: string[];
}> {
	const autoIsMac = useIsMacPlatform();
	const isMac = platform === "auto" ? autoIsMac : platform === "mac";

	return useMemo(
		() => [
			{
				label: "Search",
				keys: isMac ? ["⌘", "F"] : ["Ctrl", "F"],
			},
			{
				label: "Fold current",
				keys: isMac ? ["⌘", "⌥", "["] : ["Ctrl", "Shift", "["],
			},
			{
				label: "Unfold current",
				keys: isMac ? ["⌘", "⌥", "]"] : ["Ctrl", "Shift", "]"],
			},
			{
				label: "Fold all",
				keys: isMac ? ["⌃", "⌥", "["] : ["Ctrl", "Alt", "["],
			},
			{
				label: "Unfold all",
				keys: isMac ? ["⌃", "⌥", "]"] : ["Ctrl", "Alt", "]"],
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
						className="inline-flex items-center gap-1.5 whitespace-nowrap"
					>
						<span>{shortcut.label}</span>
						<div className="inline-flex items-center gap-1">
							{shortcut.keys.map((key, index) => (
								<div
									key={`${shortcut.label}-${key}-${String(index)}`}
									className="inline-flex items-center gap-1"
								>
									<kbd className="inline-flex min-h-6 min-w-6 items-center justify-center rounded-md border border-border bg-muted px-1.5 font-mono text-[10px] font-semibold tracking-tight text-foreground shadow-xs">
										{key}
									</kbd>
								</div>
							))}
						</div>
					</div>
				))}
			</div>
		</div>
	);
}

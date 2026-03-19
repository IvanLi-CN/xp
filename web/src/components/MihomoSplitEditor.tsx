import { yaml } from "@codemirror/lang-yaml";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { githubDark, githubLight } from "@uiw/codemirror-theme-github";
import type { CSSProperties } from "react";
import { useMemo } from "react";
import CodeMirrorMerge from "react-codemirror-merge";

import { useUiPrefsOptional } from "./UiPrefs";
import { YamlCodeEditor } from "./YamlCodeEditor";

const Original = CodeMirrorMerge.Original;
const Modified = CodeMirrorMerge.Modified;

const CODEMIRROR_BASIC_SETUP = {
	lineNumbers: true,
	highlightActiveLineGutter: true,
	foldGutter: true,
	allowMultipleSelections: true,
	indentOnInput: true,
	bracketMatching: true,
	closeBrackets: true,
	autocompletion: true,
	highlightActiveLine: true,
	highlightSelectionMatches: true,
	searchKeymap: true,
	foldKeymap: true,
	completionKeymap: true,
	tabSize: 2,
};

const IS_TEST_MODE = import.meta.env.MODE === "test";

type MihomoSplitEditorProps = {
	originalLabel: string;
	originalDescription: string;
	originalValue: string;
	onOriginalChange: (value: string) => void;
	originalPlaceholder?: string;
	modifiedLabel: string;
	modifiedDescription: string;
	modifiedValue: string;
	modifiedPlaceholder?: string;
	minRows?: number;
};

export function MihomoSplitEditor({
	originalLabel,
	originalDescription,
	originalValue,
	onOriginalChange,
	originalPlaceholder,
	modifiedLabel,
	modifiedDescription,
	modifiedValue,
	modifiedPlaceholder,
	minRows = 18,
}: MihomoSplitEditorProps) {
	const prefs = useUiPrefsOptional();
	const extensions = useMemo(() => [yaml(), EditorView.lineWrapping], []);
	const editorTheme =
		prefs?.resolvedTheme === "dark" ? githubDark : githubLight;
	const editorHeight = `${Math.max(minRows, 8) * 24}px`;

	if (IS_TEST_MODE) {
		return (
			<div className="grid gap-5 xl:grid-cols-2">
				<div className="space-y-3">
					<div className="space-y-1">
						<h3 className="text-sm font-semibold">{originalLabel}</h3>
						<p className="text-xs text-muted-foreground">
							{originalDescription}
						</p>
					</div>
					<YamlCodeEditor
						label={originalLabel}
						value={originalValue}
						onChange={onOriginalChange}
						placeholder={originalPlaceholder}
						minRows={minRows}
						hideLabel
						helperText="Input editor · line numbers · fold · Ctrl/Cmd+F"
					/>
				</div>
				<div className="space-y-3">
					<div className="space-y-1">
						<h3 className="text-sm font-semibold">{modifiedLabel}</h3>
						<p className="text-xs text-muted-foreground">
							{modifiedDescription}
						</p>
					</div>
					<YamlCodeEditor
						label={modifiedLabel}
						value={modifiedValue}
						onChange={() => {}}
						placeholder={modifiedPlaceholder}
						minRows={minRows}
						readOnly
						hideLabel
						helperText="Read-only preview · line numbers · fold · Ctrl/Cmd+F"
					/>
				</div>
			</div>
		);
	}

	return (
		<div className="space-y-3">
			<div className="grid gap-5 xl:grid-cols-2">
				<div className="space-y-1">
					<h3 className="text-sm font-semibold">{originalLabel}</h3>
					<p className="text-xs text-muted-foreground">{originalDescription}</p>
				</div>
				<div className="space-y-1">
					<h3 className="text-sm font-semibold">{modifiedLabel}</h3>
					<p className="text-xs text-muted-foreground">{modifiedDescription}</p>
				</div>
			</div>

			<div
				className="overflow-hidden rounded-2xl border border-border bg-background [&_.cm-mergeView]:h-[var(--mihomo-split-height)] [&_.cm-mergeViewEditor]:min-w-0 [&_.cm-mergeViewEditor]:h-full [&_.cm-mergeViewEditor_.cm-editor]:h-full [&_.cm-mergeViewEditor_.cm-editor]:min-h-0 [&_.cm-scroller]:h-full [&_.cm-scroller]:overflow-auto [&_.cm-scroller]:font-mono [&_.cm-scroller]:text-sm [&_.cm-content]:min-h-full [&_.cm-content]:pt-3"
				style={
					{
						"--mihomo-split-height": editorHeight,
					} as CSSProperties
				}
			>
				<CodeMirrorMerge
					orientation="a-b"
					theme={editorTheme}
					gutter
					highlightChanges={modifiedValue.length > 0}
					destroyRerender={false}
				>
					<Original
						value={originalValue}
						onChange={onOriginalChange}
						placeholder={originalPlaceholder}
						basicSetup={CODEMIRROR_BASIC_SETUP}
						extensions={extensions}
					/>
					<Modified
						value={modifiedValue}
						placeholder={modifiedPlaceholder}
						basicSetup={CODEMIRROR_BASIC_SETUP}
						readOnly
						editable={false}
						extensions={[
							...extensions,
							EditorState.readOnly.of(true),
							EditorView.editable.of(false),
						]}
					/>
				</CodeMirrorMerge>
			</div>

			<div className="grid gap-5 xl:grid-cols-2">
				<span className="text-xs opacity-70">
					Split editor · shared scroll surface · line numbers · fold ·
					Ctrl/Cmd+F
				</span>
				<span className="text-xs opacity-70">
					Read-only preview · diff highlights only after explicit execution
				</span>
			</div>
		</div>
	);
}

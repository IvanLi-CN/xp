import { yaml } from "@codemirror/lang-yaml";
import { githubDark, githubLight } from "@uiw/codemirror-theme-github";
import CodeMirror from "@uiw/react-codemirror";
import type { EditorView } from "@uiw/react-codemirror";
import { type ReactNode, useId, useMemo } from "react";

import { useUiPrefsOptional } from "./UiPrefs";
import { textareaClass } from "./ui-helpers";
import { Textarea } from "./ui/textarea";

type YamlCodeEditorProps = {
	label: string;
	value: string;
	onChange: (value: string) => void;
	placeholder?: string;
	minRows?: number;
	helperText?: ReactNode | null;
	readOnly?: boolean;
	hideLabel?: boolean;
	onCreateEditor?: (view: EditorView) => void;
};

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

export function YamlCodeEditor({
	label,
	value,
	onChange,
	placeholder,
	minRows = 8,
	helperText = "YAML syntax highlight · line numbers · fold · Ctrl/Cmd+F",
	readOnly = false,
	hideLabel = false,
	onCreateEditor,
}: YamlCodeEditorProps) {
	const prefs = useUiPrefsOptional();
	const labelId = useId();
	const editorHeight = `${Math.max(minRows, 4) * 24}px`;
	const extensions = useMemo(() => [yaml()], []);
	const editorTheme =
		prefs?.resolvedTheme === "dark" ? githubDark : githubLight;

	if (IS_TEST_MODE) {
		return (
			<div className="space-y-2">
				<span
					className={
						hideLabel ? "sr-only" : "text-sm font-medium text-foreground"
					}
				>
					{label}
				</span>
				<Textarea
					aria-label={label}
					className={textareaClass("font-mono")}
					rows={minRows}
					value={value}
					readOnly={readOnly}
					onChange={(event) => onChange(event.target.value)}
					placeholder={placeholder}
				/>
			</div>
		);
	}

	return (
		<div className="space-y-2">
			<span
				className={
					hideLabel ? "sr-only" : "text-sm font-medium text-foreground"
				}
				id={labelId}
			>
				{label}
			</span>
			<div className="overflow-hidden rounded-2xl border border-border bg-background">
				<CodeMirror
					value={value}
					height={editorHeight}
					placeholder={placeholder}
					theme={editorTheme}
					extensions={extensions}
					basicSetup={CODEMIRROR_BASIC_SETUP}
					readOnly={readOnly}
					editable={!readOnly}
					onChange={(nextValue) => onChange(nextValue)}
					onCreateEditor={(view) => onCreateEditor?.(view)}
					aria-labelledby={labelId}
					className="text-sm font-mono"
				/>
			</div>
			{helperText ? (
				<span className="text-xs opacity-70">{helperText}</span>
			) : null}
		</div>
	);
}

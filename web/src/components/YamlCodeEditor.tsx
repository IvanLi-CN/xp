import { yaml } from "@codemirror/lang-yaml";
import { githubDark, githubLight } from "@uiw/codemirror-theme-github";
import CodeMirror from "@uiw/react-codemirror";
import { useId, useMemo } from "react";
import { useUiPrefsOptional } from "./UiPrefs";

type YamlCodeEditorProps = {
	label: string;
	value: string;
	onChange: (value: string) => void;
	placeholder?: string;
	minRows?: number;
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
}: YamlCodeEditorProps) {
	const prefs = useUiPrefsOptional();
	const labelId = useId();
	const editorHeight = `${Math.max(minRows, 4) * 24}px`;
	const extensions = useMemo(() => [yaml()], []);
	const editorTheme =
		prefs?.resolvedTheme === "dark" ? githubDark : githubLight;

	if (IS_TEST_MODE) {
		return (
			<label className="form-control gap-2">
				<span className="label-text">{label}</span>
				<textarea
					className="textarea textarea-bordered font-mono"
					rows={minRows}
					value={value}
					onChange={(event) => onChange(event.target.value)}
					placeholder={placeholder}
				/>
			</label>
		);
	}

	return (
		<div className="form-control gap-2">
			<span className="label-text" id={labelId}>
				{label}
			</span>
			<div className="rounded-box border border-base-300 bg-base-100 overflow-hidden">
				<CodeMirror
					value={value}
					height={editorHeight}
					placeholder={placeholder}
					theme={editorTheme}
					extensions={extensions}
					basicSetup={CODEMIRROR_BASIC_SETUP}
					onChange={(nextValue) => onChange(nextValue)}
					aria-labelledby={labelId}
					className="text-sm font-mono"
				/>
			</div>
			<span className="text-xs opacity-70">
				YAML syntax highlight · line numbers · fold · Ctrl/Cmd+F
			</span>
		</div>
	);
}

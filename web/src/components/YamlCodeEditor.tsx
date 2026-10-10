import { yaml } from "@codemirror/lang-yaml";
import { Transaction } from "@codemirror/state";
import { githubDark, githubLight } from "@uiw/codemirror-theme-github";
import CodeMirror, {
	ExternalChange,
	type EditorView,
} from "@uiw/react-codemirror";
import { type ReactNode, useId, useLayoutEffect, useMemo, useRef } from "react";

import { cn } from "@/lib/utils";

import { recursiveFoldKeymap } from "../lib/codemirror-folding";
import { EditorShortcutHint } from "./EditorShortcutHint";
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
	showShortcutHint?: boolean;
	fillHeight?: boolean;
	preserveEditorStateOnValueChange?: boolean;
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
	helperText,
	readOnly = false,
	hideLabel = false,
	onCreateEditor,
	showShortcutHint = false,
	fillHeight = false,
	preserveEditorStateOnValueChange = false,
}: YamlCodeEditorProps) {
	const prefs = useUiPrefsOptional();
	const labelId = useId();
	const resolvedHelperText = helperText ?? null;
	const editorHeight = `${Math.max(minRows, 4) * 24}px`;
	const extensions = useMemo(() => [yaml(), recursiveFoldKeymap], []);
	const editorViewRef = useRef<EditorView | null>(null);
	const editorTheme =
		prefs?.resolvedTheme === "dark" ? githubDark : githubLight;

	useLayoutEffect(() => {
		if (!preserveEditorStateOnValueChange) return;
		const view = editorViewRef.current;
		if (!view) return;
		const currentValue = view.state.doc.toString();
		if (currentValue === value) return;

		let from = 0;
		while (
			from < currentValue.length &&
			from < value.length &&
			currentValue[from] === value[from]
		) {
			from += 1;
		}
		let currentEnd = currentValue.length;
		let nextEnd = value.length;
		while (
			currentEnd > from &&
			nextEnd > from &&
			currentValue[currentEnd - 1] === value[nextEnd - 1]
		) {
			currentEnd -= 1;
			nextEnd -= 1;
		}

		view.dispatch({
			changes: {
				from,
				to: currentEnd,
				insert: value.slice(from, nextEnd),
			},
			annotations: [
				ExternalChange.of(true),
				Transaction.addToHistory.of(false),
			],
		});
	}, [preserveEditorStateOnValueChange, value]);

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
				{showShortcutHint ? <EditorShortcutHint /> : null}
			</div>
		);
	}

	return (
		<div
			className={cn(
				"min-w-0 max-w-full",
				fillHeight ? "flex h-full min-h-0 flex-col" : "space-y-2",
			)}
		>
			<span
				className={
					hideLabel ? "sr-only" : "text-sm font-medium text-foreground"
				}
				id={labelId}
			>
				{label}
			</span>
			<div
				className={cn(
					"min-w-0 max-w-full overflow-hidden",
					fillHeight
						? "flex min-h-0 flex-1 flex-col rounded-none border-0 bg-transparent"
						: "rounded-2xl border border-border bg-background",
				)}
			>
				<CodeMirror
					value={value}
					height={fillHeight ? "100%" : editorHeight}
					placeholder={placeholder}
					theme={editorTheme}
					extensions={extensions}
					basicSetup={CODEMIRROR_BASIC_SETUP}
					readOnly={readOnly}
					editable={!readOnly}
					onChange={(nextValue) => onChange(nextValue)}
					onCreateEditor={(view) => {
						editorViewRef.current = view;
						onCreateEditor?.(view);
					}}
					aria-labelledby={labelId}
					className={cn(
						"min-w-0 max-w-full text-sm font-mono",
						fillHeight &&
							"min-h-0 flex-1 [&_.cm-editor]:h-full [&_.cm-scroller]:h-full",
					)}
				/>
			</div>
			{resolvedHelperText ? (
				<span className="text-xs opacity-70">{resolvedHelperText}</span>
			) : null}
			{showShortcutHint ? (
				<div className="pt-1">
					<EditorShortcutHint />
				</div>
			) : null}
		</div>
	);
}

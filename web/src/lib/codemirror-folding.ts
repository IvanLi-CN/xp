import { foldEffect, foldNodeProp, syntaxTree } from "@codemirror/language";
import { type EditorState, Prec } from "@codemirror/state";
import { type EditorView, keymap } from "@codemirror/view";

type FoldRange = { from: number; to: number };

export function collectRecursiveFoldRanges(state: EditorState): FoldRange[] {
	const ranges = new Map<string, FoldRange>();

	syntaxTree(state).iterate({
		enter(node) {
			const fold = node.type.prop(foldNodeProp);
			if (!fold) return;

			const range = fold(node.node, state);
			if (!range || range.from >= range.to) return;

			ranges.set(`${range.from}:${range.to}`, range);
		},
	});

	return [...ranges.values()].sort(
		(left, right) => left.from - right.from || right.to - left.to,
	);
}

export function foldAllRecursive(view: EditorView): boolean {
	const ranges = collectRecursiveFoldRanges(view.state);
	if (ranges.length === 0) return false;

	view.dispatch({
		effects: ranges.map((range) => foldEffect.of(range)),
	});
	return true;
}

// Keep the standard fold/unfold bindings, but make the "fold all" binding
// create independent fold state for every nested YAML object.
export const recursiveFoldKeymap = Prec.high(
	keymap.of([{ key: "Ctrl-Alt-[", run: foldAllRecursive }]),
);

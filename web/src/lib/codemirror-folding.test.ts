import { yaml } from "@codemirror/lang-yaml";
import { codeFolding, foldedRanges, unfoldEffect } from "@codemirror/language";
import { EditorState } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import { describe, expect, it } from "vitest";

import {
	collectRecursiveFoldRanges,
	foldAllRecursive,
} from "./codemirror-folding";

describe("collectRecursiveFoldRanges", () => {
	it("collects nested YAML objects while ignoring scalar pairs", () => {
		const source = [
			"proxy-groups:",
			"  - name: high",
			"    type: select",
			"    use:",
			"      - xp-system-generated",
			"      - PQ",
			"    proxies:",
			"      - Japan",
			"      - HongKong",
			"  - name: other",
			"    type: select",
			"    proxies:",
			"      - US",
		].join("\n");
		const state = EditorState.create({ doc: source, extensions: [yaml()] });

		const ranges = collectRecursiveFoldRanges(state);

		expect(ranges).toEqual([
			{ from: 13, to: source.length },
			{ from: 28, to: 137 },
			{ from: 54, to: 93 },
			{ from: 106, to: 137 },
			{ from: 153, to: source.length },
			{ from: 183, to: source.length },
		]);
	});

	it("keeps child folds when the parent is unfolded", () => {
		const source = [
			"proxy-groups:",
			"  - name: high",
			"    use:",
			"      - xp-system-generated",
			"    proxies:",
			"      - Japan",
		].join("\n");
		let state = EditorState.create({
			doc: source,
			extensions: [yaml(), codeFolding()],
		});
		const view = {
			get state() {
				return state;
			},
			dispatch(transaction: Parameters<typeof state.update>[0]) {
				state = state.update(transaction).state;
			},
		} as unknown as EditorView;

		expect(foldAllRecursive(view)).toBe(true);
		const foldedBefore: Array<{ from: number; to: number }> = [];
		foldedRanges(state).between(0, state.doc.length, (from, to) => {
			foldedBefore.push({ from, to });
		});
		const parent = foldedBefore[0];
		expect(parent).toBeDefined();
		if (!parent)
			throw new Error("recursive fold did not create a parent range");
		expect(foldedBefore.length).toBeGreaterThan(1);

		state = state.update({ effects: unfoldEffect.of(parent) }).state;
		const foldedAfter: Array<{ from: number; to: number }> = [];
		foldedRanges(state).between(0, state.doc.length, (from, to) => {
			foldedAfter.push({ from, to });
		});

		expect(foldedAfter).toEqual(foldedBefore.slice(1));
		expect(foldedAfter).not.toContainEqual(parent);
	});
});

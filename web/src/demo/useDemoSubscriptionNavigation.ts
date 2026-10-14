import { useRouter, useRouterState } from "@tanstack/react-router";
import { useEffect, useRef } from "react";

import {
	DEFAULT_SUBSCRIPTION_FORMAT,
	type SubscriptionFormat,
} from "../api/subscription";
import { isDemoRuntime } from "./runtimeMode";

type PreviewOptions = {
	userId: string;
	subscriptionToken: string;
	setFormat: (format: SubscriptionFormat) => void;
	setMirror: (mirror: boolean) => void;
	setOpen: (open: boolean) => void;
	loadPreview: (
		token: string,
		format: SubscriptionFormat,
		mirror: boolean,
		formatError: (error: unknown) => string,
	) => Promise<void>;
	formatError: (error: unknown) => string;
};

export function useDemoSubscriptionNavigation(options: PreviewOptions) {
	const router = useRouter();
	const search = useRouterState({
		select: (state) => state.location.searchStr,
	});
	const loaded = useRef<string | null>(null);
	const previousUser = useRef(options.userId);
	const {
		userId,
		subscriptionToken,
		setFormat,
		setMirror,
		setOpen,
		loadPreview,
		formatError,
	} = options;

	useEffect(() => {
		if (!isDemoRuntime()) return;
		const params = new URLSearchParams(search);
		const value = params.get("d_format");
		const format: SubscriptionFormat =
			value === "raw" || value === "clash" || value === "mihomo"
				? value
				: DEFAULT_SUBSCRIPTION_FORMAT;
		const mirror = ["1", "mirror"].includes(params.get("d_mirror") ?? "");
		setFormat(format);
		setMirror(mirror);
		if (previousUser.current !== userId) {
			previousUser.current = userId;
			loaded.current = null;
		}
		const open = ["1", "preview"].includes(params.get("d_preview") ?? "");
		setOpen(open);
		if (!open || !subscriptionToken) return;
		const key = `${userId}:${subscriptionToken}:${format}:${mirror}`;
		if (loaded.current === key) return;
		loaded.current = key;
		void loadPreview(subscriptionToken, format, mirror, formatError);
	}, [
		search,
		userId,
		subscriptionToken,
		setFormat,
		setMirror,
		setOpen,
		loadPreview,
		formatError,
	]);

	function updateSearch(
		format: SubscriptionFormat,
		mirror: boolean,
		preview = false,
	) {
		if (!isDemoRuntime()) return;
		const location = router.state.location;
		const alreadyOpen = ["1", "preview"].includes(
			new URLSearchParams(location.searchStr).get("d_preview") ?? "",
		);
		void router.navigate({
			search: (current) =>
				({
					...current,
					d_format: format,
					d_mirror: mirror ? "mirror" : undefined,
					d_preview: preview ? "preview" : undefined,
				}) as never,
			replace: preview && alreadyOpen,
			state: (current) =>
				preview && alreadyOpen
					? current
					: {
							...current,
							xpDemoPreviewParentIndex: preview
								? location.state.__TSR_index
								: undefined,
							xpDemoPreviewUserId: preview ? userId : undefined,
						},
		});
	}

	function closePreview() {
		if (!isDemoRuntime()) {
			setOpen(false);
			return;
		}
		const { state } = router.state.location;
		if (
			"xpDemoPreviewUserId" in state &&
			state.xpDemoPreviewUserId === userId &&
			"xpDemoPreviewParentIndex" in state &&
			state.xpDemoPreviewParentIndex === state.__TSR_index - 1
		) {
			router.history.back();
			return;
		}
		void router.navigate({
			search: (current) => ({ ...current, d_preview: undefined }) as never,
			replace: true,
			state: (current) => ({
				...current,
				xpDemoPreviewParentIndex: undefined,
				xpDemoPreviewUserId: undefined,
			}),
		});
	}

	return { updateSearch, closePreview };
}

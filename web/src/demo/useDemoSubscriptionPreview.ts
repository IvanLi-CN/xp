import { useEffect, useState } from "react";

import {
	DEFAULT_SUBSCRIPTION_FORMAT,
	type SubscriptionFormat,
} from "@/api/subscription";

import { subscriptionUrl } from "./format";
import { fetchDemoSubscription } from "./mockApi";
import type { DemoState, DemoUser } from "./types";

type UseDemoSubscriptionPreviewOptions = {
	state: DemoState;
	user: DemoUser | undefined;
};

export function useDemoSubscriptionPreview({
	state,
	user,
}: UseDemoSubscriptionPreviewOptions) {
	const [subscriptionFormat, setSubscriptionFormat] =
		useState<SubscriptionFormat>(DEFAULT_SUBSCRIPTION_FORMAT);
	const [useExternalResourceMirror, setUseExternalResourceMirror] =
		useState(false);
	const [subscriptionOpen, setSubscriptionOpen] = useState(false);
	const [subscriptionLoading, setSubscriptionLoading] = useState(false);
	const [subscriptionText, setSubscriptionText] = useState("");
	const [subscriptionError, setSubscriptionError] = useState<string | null>(
		null,
	);

	const userId = user?.id;
	useEffect(() => {
		if (userId !== undefined) setUseExternalResourceMirror(false);
	}, [userId]);

	async function fetchSubscriptionPreview(
		nextFormat = subscriptionFormat,
		nextMirror = useExternalResourceMirror,
	) {
		if (!user) return;
		setSubscriptionOpen(true);
		setSubscriptionFormat(nextFormat);
		setUseExternalResourceMirror(nextMirror);
		setSubscriptionLoading(true);
		setSubscriptionError(null);
		try {
			setSubscriptionText(
				await fetchDemoSubscription(state, user, nextFormat, nextMirror),
			);
		} catch (error) {
			setSubscriptionError(
				error instanceof Error ? error.message : "Failed to load subscription.",
			);
			setSubscriptionText("");
		} finally {
			setSubscriptionLoading(false);
		}
	}

	function handleExternalResourceMirrorChange(checked: boolean) {
		setUseExternalResourceMirror(checked);
		if (subscriptionOpen && subscriptionFormat === "mihomo") {
			void fetchSubscriptionPreview("mihomo", checked);
		}
	}

	const subscriptionPreviewUrl = user
		? subscriptionUrl(
				user.subscriptionToken,
				subscriptionFormat,
				useExternalResourceMirror,
			)
		: "";

	return {
		subscriptionFormat,
		setSubscriptionFormat,
		subscriptionOpen,
		setSubscriptionOpen,
		subscriptionLoading,
		subscriptionText,
		subscriptionError,
		useExternalResourceMirror,
		fetchSubscriptionPreview,
		handleExternalResourceMirrorChange,
		subscriptionPreviewUrl,
	};
}

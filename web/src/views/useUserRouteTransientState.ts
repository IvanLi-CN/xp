import { useEffect, useRef, useState } from "react";

import {
	type SubscriptionFormat,
	fetchSubscription,
} from "../api/subscription";

export function useUserRouteTransientState(userId: string) {
	const [resetTokenOpen, setResetTokenOpen] = useState(false);
	const [isResettingToken, setIsResettingToken] = useState(false);
	const [resetCredentialsOpen, setResetCredentialsOpen] = useState(false);
	const [isResettingCredentials, setIsResettingCredentials] = useState(false);
	const [subOpen, setSubOpen] = useState(false);
	const [subLoading, setSubLoading] = useState(false);
	const [subText, setSubText] = useState("");
	const [subError, setSubError] = useState<string | null>(null);
	const [deleteOpen, setDeleteOpen] = useState(false);
	const [isDeleting, setIsDeleting] = useState(false);
	const subscriptionPreviewRequestRef = useRef(0);
	const currentUserIdRef = useRef(userId);
	currentUserIdRef.current = userId;
	const [stateUserId, setStateUserId] = useState(userId);

	useEffect(() => {
		subscriptionPreviewRequestRef.current += 1;
		setStateUserId(userId);
		setSubOpen(false);
		setSubLoading(false);
		setSubText("");
		setResetTokenOpen(false);
		setIsResettingToken(false);
		setResetCredentialsOpen(false);
		setIsResettingCredentials(false);
		setDeleteOpen(false);
		setIsDeleting(false);
	}, [userId]);

	async function loadSubscriptionPreview(
		subscriptionToken: string,
		nextFormat: SubscriptionFormat,
		nextMirror: boolean,
		formatError: (error: unknown) => string,
	) {
		if (!subscriptionToken) return;
		const previewUserId = userId;
		const requestId = ++subscriptionPreviewRequestRef.current;
		setSubLoading(true);
		setSubError(null);
		try {
			const text =
				nextFormat === "mihomo" && nextMirror
					? await fetchSubscription(subscriptionToken, nextFormat, "mirror")
					: await fetchSubscription(subscriptionToken, nextFormat);
			if (
				requestId !== subscriptionPreviewRequestRef.current ||
				currentUserIdRef.current !== previewUserId
			)
				return;
			setSubText(text);
		} catch (error) {
			if (
				requestId !== subscriptionPreviewRequestRef.current ||
				currentUserIdRef.current !== previewUserId
			)
				return;
			setSubError(formatError(error));
			setSubText("");
		} finally {
			if (
				requestId === subscriptionPreviewRequestRef.current &&
				currentUserIdRef.current === previewUserId
			) {
				setSubLoading(false);
			}
		}
	}

	return {
		currentUserIdRef,
		deleteOpen,
		isCurrentTransientState: stateUserId === userId,
		isDeleting,
		isResettingCredentials,
		isResettingToken,
		loadSubscriptionPreview,
		resetCredentialsOpen,
		resetTokenOpen,
		setDeleteOpen,
		setIsDeleting,
		setIsResettingCredentials,
		setIsResettingToken,
		setResetCredentialsOpen,
		setResetTokenOpen,
		setSubOpen,
		subError,
		subLoading,
		subOpen,
		subText,
	};
}

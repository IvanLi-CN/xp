import { Link } from "@tanstack/react-router";

import { isBackendApiError } from "@/api/backendError";

import { resolveLoginRedirectFromHref } from "../utils/navigation";
import { Button } from "./Button";

type AuthRecoveryActionProps = {
	error: unknown;
};

export function isUnauthorizedError(error: unknown): boolean {
	return isBackendApiError(error) && error.status === 401;
}

export function AuthRecoveryAction({ error }: AuthRecoveryActionProps) {
	if (!isUnauthorizedError(error)) return null;

	const loginRedirect = resolveLoginRedirectFromHref(window.location.href);
	return (
		<Button asChild variant="secondary">
			<Link to="/login" search={{ redirect: loginRedirect }}>
				Sign in
			</Link>
		</Button>
	);
}

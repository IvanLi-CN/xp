import type { ReactNode } from "react";

import { PageState } from "./PageState";
import { readAdminToken } from "./auth";

type AuthGateProps = {
	children: ReactNode;
	fallback?: ReactNode;
};

export function AuthGate({ children, fallback }: AuthGateProps) {
	const hasToken = readAdminToken().length > 0;

	if (!hasToken) {
		return (
			fallback ?? (
				<PageState
					variant="empty"
					title="Authentication required"
					description="Please sign in with an admin token to continue."
				/>
			)
		);
	}

	return <>{children}</>;
}

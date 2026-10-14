import type { ReactNode } from "react";
import { vi } from "vitest";

/** Static routing for live-page unit tests; Demo history is covered in E2E. */
export function userDetailsRouterMock(getUserId: () => string) {
	const navigate = vi.fn();
	const state = { location: { searchStr: "" } };
	return {
		Link: ({ children, to, ...rest }: { children: ReactNode; to?: string }) => (
			<a href={to ?? "#"} {...rest}>
				{children}
			</a>
		),
		useNavigate: () => navigate,
		useParams: () => ({ userId: getUserId() }),
		useRouter: () => ({ navigate, state, history: { back: vi.fn() } }),
		useRouterState: <T,>({ select }: { select: (value: typeof state) => T }) =>
			select(state),
	};
}

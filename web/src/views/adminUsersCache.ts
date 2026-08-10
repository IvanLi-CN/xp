import type { AdminUser, AdminUsersResponse } from "../api/adminUsers";

export function appendAdminUser(
	previous: AdminUsersResponse | undefined,
	user: AdminUser,
) {
	return previous
		? { ...previous, items: [...previous.items, user] }
		: previous;
}

export function replaceAdminUser(
	previous: AdminUsersResponse | undefined,
	user: AdminUser,
) {
	return previous
		? {
				...previous,
				items: previous.items.map((item) =>
					item.user_id === user.user_id ? user : item,
				),
			}
		: previous;
}

export function removeAdminUser(
	previous: AdminUsersResponse | undefined,
	userId: string,
) {
	return previous
		? {
				...previous,
				items: previous.items.filter((item) => item.user_id !== userId),
			}
		: previous;
}

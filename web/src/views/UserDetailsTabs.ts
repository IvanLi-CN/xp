export type UserDetailsTab =
	| "user"
	| "access"
	| "quotaStatus"
	| "traffic"
	| "usageDetails";

export const USER_TAB_OPTIONS: Array<{
	value: UserDetailsTab;
	label: string;
}> = [
	{ value: "user", label: "User" },
	{ value: "access", label: "Access" },
	{ value: "quotaStatus", label: "Quota status" },
	{ value: "traffic", label: "Traffic" },
	{ value: "usageDetails", label: "Usage details" },
];

import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useEffect, useState } from "react";

import { readAdminToken } from "./auth";

export function useActiveAdminSession(): string {
	const queryClient = useQueryClient();
	const navigate = useNavigate();
	const [initialToken] = useState(() => readAdminToken());
	const token = readAdminToken() === initialToken ? initialToken : "";
	useEffect(() => {
		if (!initialToken || token) return;
		void queryClient.cancelQueries();
		void navigate({ to: "/login" });
	}, [initialToken, navigate, queryClient, token]);
	return token;
}

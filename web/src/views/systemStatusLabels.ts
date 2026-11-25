import type { AdminMeshPeer } from "@/api/adminMesh";

export function directValidationLabel(peer: AdminMeshPeer) {
	return {
		configured_unverified: "Admission not verified",
		verified: "Admission verified",
		transport_failed: "Admission transport failed",
		protocol_rejected: "Admission protocol rejected",
	}[peer.direct_validation ?? "configured_unverified"];
}

export function peerConnectivityLabels(
	peer: Pick<AdminMeshPeer, "direct_connectivity" | "public_fallback_health">,
) {
	return {
		direct: {
			verified: "Direct verified",
			unavailable: "Direct unavailable",
			not_applicable: "Direct not applicable",
			unknown: "Direct unknown",
		}[peer.direct_connectivity],
		public: {
			available: "Public fallback available",
			unavailable: "Public fallback unavailable",
			unknown: "Public fallback unknown",
		}[peer.public_fallback_health],
	};
}

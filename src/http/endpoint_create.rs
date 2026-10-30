use super::*;

pub(super) async fn admin_create_endpoint(
    Extension(state): Extension<AppState>,
    ApiJson(req): ApiJson<CreateEndpointRequest>,
) -> Result<Json<Endpoint>, ApiError> {
    let _xmux_guard = if matches!(
        &req,
        CreateEndpointRequest::VlessRealityVisionTcp {
            mihomo_xmux: Some(_),
            ..
        }
    ) {
        join_capability::lock_xmux_settings_change(&state).await?
    } else {
        None
    };
    let endpoint = match req {
        CreateEndpointRequest::VlessRealityVisionTcp {
            node_id,
            port,
            reality,
            canary_upstream,
            accepted_authorities,
            mihomo_smux,
            mihomo_xmux,
            transport,
        } => {
            let mihomo_smux = mihomo_smux.unwrap_or_default();
            mihomo_smux.validate().map_err(ApiError::invalid_request)?;
            if let Some(xmux) = &mihomo_xmux {
                xmux.validate().map_err(ApiError::invalid_request)?;
            }
            let mihomo_xmux = mihomo_xmux.filter(|value| *value != Default::default());
            let transport = transport.unwrap_or(crate::protocol::VlessRealityTransport::Xhttp);
            let store = state.store.lock().await;
            if let Some(reality) = reality {
                if canary_upstream.is_some() {
                    return Err(ApiError::invalid_request(
                        "canary_upstream is only editable on managed VLESS endpoints",
                    ));
                }
                if accepted_authorities.is_some() {
                    return Err(ApiError::invalid_request(
                        "accepted_authorities is only editable on managed VLESS endpoints",
                    ));
                }
                store.build_endpoint(
                    node_id,
                    crate::domain::EndpointKind::VlessRealityVisionTcp,
                    port,
                    json!({
                        "reality": reality,
                        "mihomo_smux": mihomo_smux,
                        "mihomo_xmux": mihomo_xmux,
                        "transport": transport
                    }),
                )?
            } else {
                let node = store.get_node(&node_id).ok_or_else(|| {
                    ApiError::invalid_request(format!("node not found: {node_id}"))
                })?;
                let reality =
                    managed_vless_reality_for_node(&state, &node, DEFAULT_VLESS_FINGERPRINT)?;
                let spec = DefaultVlessEndpointSpec {
                    port,
                    reality_dest: reality.dest,
                    server_names: reality.server_names,
                    server_names_source: reality.server_names_source,
                    fingerprint: reality.fingerprint,
                };
                let mut endpoint =
                    build_managed_default_vless_endpoint(&spec, node_id).map_err(|e| {
                        ApiError::invalid_request(format!(
                            "failed to create managed VLESS endpoint: {e}"
                        ))
                    })?;
                let mut meta: VlessRealityVisionTcpEndpointMeta =
                    serde_json::from_value(endpoint.meta.clone())
                        .map_err(|e| ApiError::internal(e.to_string()))?;
                meta.canary_upstream = canary_upstream.map(|mut upstream| {
                    upstream.url = upstream.url.trim().to_string();
                    upstream
                });
                meta.accepted_authorities =
                    normalize_accepted_authorities(accepted_authorities.as_deref().unwrap_or(&[]))
                        .map_err(|(reason, authority)| {
                            ApiError::invalid_request(format!(
                                "invalid accepted_authority: {authority} ({reason})"
                            ))
                        })?;
                meta.mihomo_smux = mihomo_smux;
                meta.mihomo_xmux = mihomo_xmux;
                meta.transport = transport;
                endpoint.meta =
                    serde_json::to_value(meta).map_err(|e| ApiError::internal(e.to_string()))?;
                endpoint
            }
        }
        CreateEndpointRequest::Ss2022_2022Blake3Aes128Gcm {
            node_id,
            port,
            canary_upstream,
            accepted_authorities,
            mihomo_smux,
        } => {
            if canary_upstream.is_some() {
                return Err(ApiError::invalid_request(
                    "canary_upstream is only supported for vless endpoints",
                ));
            }
            if accepted_authorities.is_some() {
                return Err(ApiError::invalid_request(
                    "accepted_authorities is only supported for managed vless endpoints",
                ));
            }
            let mihomo_smux = mihomo_smux.unwrap_or_default();
            mihomo_smux.validate().map_err(ApiError::invalid_request)?;
            let store = state.store.lock().await;
            store.build_endpoint(
                node_id,
                crate::domain::EndpointKind::Ss2022_2022Blake3Aes128Gcm,
                port,
                json!({ "mihomo_smux": mihomo_smux }),
            )?
        }
    };
    let _ = raft_write(
        &state,
        crate::state::DesiredStateCommand::UpsertEndpoint {
            endpoint: endpoint.clone(),
            expected: None,
        },
    )
    .await?;
    state.reconcile.request_full();
    Ok(Json(endpoint))
}

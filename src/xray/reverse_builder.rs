use crate::{
    protocol::VLESS_XHTTP_PATH,
    xray::{builder, proto::xray},
};

pub(crate) const REVERSE_XHTTP_MAX_CONNECTIONS: i32 = 2;

pub(crate) fn reverse_xhttp_transport_settings() -> xray::transport::internet::TransportConfig {
    let xhttp = xray::transport::internet::splithttp::Config {
        path: VLESS_XHTTP_PATH.to_string(),
        mode: "stream-one".to_string(),
        xmux: Some(xray::transport::internet::splithttp::XmuxConfig {
            max_connections: Some(xray::transport::internet::splithttp::RangeConfig {
                from: REVERSE_XHTTP_MAX_CONNECTIONS,
                to: REVERSE_XHTTP_MAX_CONNECTIONS,
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    xray::transport::internet::TransportConfig {
        protocol_name: "splithttp".to_string(),
        settings: Some(builder::to_typed_message(
            "xray.transport.internet.splithttp.Config",
            &xhttp,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{Endpoint, EndpointKind},
        xray::builder::ReverseVlessEndpoint,
    };
    use prost::Message;

    fn decode_typed<T: Message + Default>(message: &xray::common::serial::TypedMessage) -> T {
        T::decode(message.value.as_slice()).expect("decode typed message")
    }

    #[test]
    fn reverse_vless_xhttp_caps_xmux_connections() {
        let endpoint = Endpoint {
            endpoint_id: xp_test_fixtures::label_e3().to_owned(),
            node_id: xp_test_fixtures::subscription_node_n1().to_owned(),
            tag: xp_test_fixtures::label_vless_e3().to_owned(),
            kind: EndpointKind::VlessRealityVisionTcp,
            port: 443,
            meta: serde_json::json!({
                "reality": xp_test_fixtures::endpoint_reality(),
                "reality_keys": xp_test_fixtures::endpoint_reality_keys(),
                "short_ids": xp_test_fixtures::endpoint_short_ids(),
                "active_short_id": xp_test_fixtures::endpoint_active_short_id(),
                "transport": "xhttp"
            }),
        };
        let request = builder::build_reverse_vless_outbound_request(
            "reverse-outbound",
            "reverse-user",
            "00000000-0000-4000-8000-000000000001",
            &ReverseVlessEndpoint {
                access_host: xp_test_fixtures::primary_host().to_owned(),
                endpoint,
                target_port: 443,
                target_public_key_b64url_nopad: "Pf8FreUQ5qeklEqp0sUrQPztRLmqQacHXfCfhxmmKm4"
                    .to_string(),
                target_short_id_hex: "0123456789abcdef".to_string(),
                server_name: "www.example.com".to_string(),
            },
        )
        .unwrap();
        let sender: xray::app::proxyman::SenderConfig =
            decode_typed(&request.outbound.unwrap().sender_settings.unwrap());
        let transport = &sender.stream_settings.unwrap().transport_settings[0];
        let xhttp: xray::transport::internet::splithttp::Config =
            decode_typed(transport.settings.as_ref().unwrap());
        let max_connections = xhttp.xmux.unwrap().max_connections.unwrap();
        assert_eq!(max_connections.from, REVERSE_XHTTP_MAX_CONNECTIONS);
        assert_eq!(max_connections.to, REVERSE_XHTTP_MAX_CONNECTIONS);
    }
}

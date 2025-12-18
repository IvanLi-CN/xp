pub mod xray {
    pub mod app {
        pub mod proxyman {
            tonic::include_proto!("xray.app.proxyman");

            pub mod command {
                tonic::include_proto!("xray.app.proxyman.command");
            }
        }

        pub mod stats {
            pub mod command {
                tonic::include_proto!("xray.app.stats.command");
            }
        }
    }

    pub mod common {
        pub mod net {
            tonic::include_proto!("xray.common.net");
        }

        pub mod protocol {
            tonic::include_proto!("xray.common.protocol");
        }

        pub mod serial {
            tonic::include_proto!("xray.common.serial");
        }
    }

    pub mod core {
        tonic::include_proto!("xray.core");
    }

    pub mod proxy {
        pub mod shadowsocks_2022 {
            tonic::include_proto!("xray.proxy.shadowsocks_2022");
        }

        pub mod vless {
            tonic::include_proto!("xray.proxy.vless");

            pub mod inbound {
                tonic::include_proto!("xray.proxy.vless.inbound");
            }
        }
    }

    pub mod transport {
        pub mod internet {
            tonic::include_proto!("xray.transport.internet");

            pub mod reality {
                tonic::include_proto!("xray.transport.internet.reality");
            }

            pub mod tcp {
                tonic::include_proto!("xray.transport.internet.tcp");
            }
        }
    }
}

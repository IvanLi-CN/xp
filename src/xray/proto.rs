pub mod xray {
    pub mod app {
        pub mod proxyman {
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
}

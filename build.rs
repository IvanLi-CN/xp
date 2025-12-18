use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protoc_path: PathBuf = protoc_bin_vendored::protoc_bin_path()?;
    unsafe {
        std::env::set_var("PROTOC", protoc_path);
    }

    let proto_root = "proto/xray";
    let protos = [
        "proto/xray/app/proxyman/config.proto",
        "proto/xray/app/proxyman/command/command.proto",
        "proto/xray/app/stats/command/command.proto",
        "proto/xray/common/net/address.proto",
        "proto/xray/common/net/network.proto",
        "proto/xray/common/net/port.proto",
        "proto/xray/common/protocol/user.proto",
        "proto/xray/common/serial/typed_message.proto",
        "proto/xray/core/config.proto",
        "proto/xray/proxy/shadowsocks_2022/config.proto",
        "proto/xray/proxy/vless/account.proto",
        "proto/xray/proxy/vless/inbound/config.proto",
        "proto/xray/transport/internet/config.proto",
        "proto/xray/transport/internet/reality/config.proto",
        "proto/xray/transport/internet/tcp/config.proto",
    ];

    for proto in protos {
        println!("cargo:rerun-if-changed={proto}");
    }

    tonic_prost_build::configure()
        .build_client(true)
        .build_server(true)
        .compile_protos(&protos, &[proto_root])?;

    Ok(())
}

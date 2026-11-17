from pathlib import Path
p=Path('source/crates/xp-proxy/src/main.rs')
s=p.read_text()
# Change only socket options, preserving protocol and lifecycle code.
s=s.replace('let (local, _) = listener.accept().await?;', 'let (local, _) = listener.accept().await?;\n        local.set_nodelay(true)?;')
s=s.replace('    let (size, probe) = peek_probe(&stream).await?;', '    stream.set_nodelay(true)?;\n    let (size, probe) = peek_probe(&stream).await?;')
s=s.replace('    let tls = reality_tls::connect(\n        TcpStream::connect(config.remote).await?,', '    let remote = TcpStream::connect(config.remote).await?;\n    remote.set_nodelay(true)?;\n    let tls = reality_tls::connect(\n        remote,')
p.write_text(s)

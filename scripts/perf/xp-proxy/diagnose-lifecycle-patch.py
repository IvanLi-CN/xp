from pathlib import Path
p = Path('source/crates/xp-proxy/src/main.rs')
s = p.read_text()
s = s.replace(
    '    tokio::spawn(async move {\n        let _ = connection.await;\n    });',
    '    let mut connection_tasks = tokio::task::JoinSet::new();\n'
    '    connection_tasks.spawn(async move {\n        let _ = connection.await;\n    });',
    1,
)
s = s.replace(
    '    tokio::spawn(async move {\n        let _ = uplink.await;\n    });\n'
    '    let mut response = response.await?;',
    '    let downlink = async move {\n    let mut response = response.await?;',
    1,
)
s = s.replace(
    '    downlink.await?;\n    Ok(())\n}\n\nasync fn handle_connection',
    '    downlink.await\n    };\n    tokio::try_join!(uplink, downlink)?;\n'
    '    Ok(())\n}\n\nasync fn handle_connection',
    1,
)
p.write_text(s)

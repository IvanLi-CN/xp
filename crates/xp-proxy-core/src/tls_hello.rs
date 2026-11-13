#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientHelloInfo {
    pub record: Vec<u8>,
    pub handshake: Vec<u8>,
    pub session_id_offset: usize,
    pub random: [u8; 32],
    pub session_id: Vec<u8>,
    pub cipher_suites: Vec<u16>,
    pub server_name: Option<String>,
    pub alpn: Vec<String>,
    pub signature_schemes: Option<Vec<u16>>,
    pub x25519_key_share: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerHelloInfo {
    pub record: Vec<u8>,
    pub handshake: Vec<u8>,
    pub random: [u8; 32],
    pub cipher_suite: u16,
    pub x25519_key_share: Option<Vec<u8>>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ClientHelloError {
    #[error("TLS record is truncated")]
    Truncated,
    #[error("unexpected TLS record or handshake type")]
    UnexpectedType,
    #[error("malformed ClientHello")]
    Malformed,
    #[error("unsupported ClientHello session id length")]
    SessionIdTooLong,
}

pub fn parse_client_hello(input: &[u8]) -> Result<ClientHelloInfo, ClientHelloError> {
    if input.len() < 5 {
        return Err(ClientHelloError::Truncated);
    }
    let mut record_end = 0usize;
    let mut handshake = Vec::new();
    let hello_len = loop {
        if input.len() < record_end + 5 {
            return Err(ClientHelloError::Truncated);
        }
        let record_header = &input[record_end..record_end + 5];
        if record_header[0] != 0x16 || record_header[1] != 0x03 {
            return Err(ClientHelloError::UnexpectedType);
        }
        let record_len = u16::from_be_bytes([record_header[3], record_header[4]]) as usize;
        let payload_start = record_end + 5;
        let next_record_end = payload_start
            .checked_add(record_len)
            .ok_or(ClientHelloError::Malformed)?;
        if input.len() < next_record_end {
            return Err(ClientHelloError::Truncated);
        }
        handshake.extend_from_slice(&input[payload_start..next_record_end]);
        record_end = next_record_end;
        if handshake.len() < 4 {
            continue;
        }
        if handshake[0] != 1 {
            return Err(ClientHelloError::UnexpectedType);
        }
        let hello_len = u32::from_be_bytes([0, handshake[1], handshake[2], handshake[3]]) as usize;
        if hello_len + 4 > handshake.len() {
            continue;
        }
        break hello_len;
    };
    let hello = &handshake[4..4 + hello_len];
    if hello.len() < 34 {
        return Err(ClientHelloError::Malformed);
    }
    let mut random = [0u8; 32];
    random.copy_from_slice(&hello[2..34]);
    let mut cursor = 34;
    let session_len = *hello.get(cursor).ok_or(ClientHelloError::Malformed)? as usize;
    cursor += 1;
    if session_len > 32 {
        return Err(ClientHelloError::SessionIdTooLong);
    }
    let session_id = hello
        .get(cursor..cursor + session_len)
        .ok_or(ClientHelloError::Truncated)?
        .to_vec();
    cursor += session_len;
    let cipher_len = read_u16(hello, &mut cursor)?;
    if !cipher_len.is_multiple_of(2) {
        return Err(ClientHelloError::Malformed);
    }
    let cipher_end = cursor
        .checked_add(cipher_len)
        .ok_or(ClientHelloError::Malformed)?;
    if cipher_end > hello.len() {
        return Err(ClientHelloError::Truncated);
    }
    let cipher_suites = hello[cursor..cipher_end]
        .chunks_exact(2)
        .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
        .collect();
    cursor = cipher_end;
    let compression_len = *hello.get(cursor).ok_or(ClientHelloError::Truncated)? as usize;
    cursor += 1 + compression_len;
    let extensions_len = read_u16(hello, &mut cursor)?;
    let extensions_end = cursor
        .checked_add(extensions_len)
        .ok_or(ClientHelloError::Malformed)?;
    if extensions_end > hello.len() {
        return Err(ClientHelloError::Truncated);
    }
    let mut server_name = None;
    let mut alpn = Vec::new();
    let mut signature_schemes = None;
    let mut x25519_key_share = None;
    while cursor < extensions_end {
        let extension_type = read_u16(hello, &mut cursor)?;
        let extension_len = read_u16(hello, &mut cursor)?;
        let extension_end = cursor
            .checked_add(extension_len)
            .ok_or(ClientHelloError::Malformed)?;
        if extension_end > extensions_end {
            return Err(ClientHelloError::Truncated);
        }
        let extension = &hello[cursor..extension_end];
        match extension_type {
            0x0000 => server_name = parse_server_name(extension),
            0x000d => signature_schemes = parse_signature_schemes(extension),
            0x0010 => alpn = parse_alpn(extension),
            0x0033 => x25519_key_share = parse_x25519_key_share(extension),
            _ => {}
        }
        cursor = extension_end;
    }
    Ok(ClientHelloInfo {
        record: input[..record_end].to_vec(),
        handshake: handshake[..4 + hello_len].to_vec(),
        session_id_offset: 39,
        random,
        session_id,
        cipher_suites,
        server_name,
        alpn,
        signature_schemes,
        x25519_key_share,
    })
}

pub fn parse_server_hello(input: &[u8]) -> Result<ServerHelloInfo, ClientHelloError> {
    let (record, handshake) = collect_handshake(input, 2)?;
    let body = &handshake[4..];
    if body.len() < 38 {
        return Err(ClientHelloError::Malformed);
    }
    let mut random = [0u8; 32];
    random.copy_from_slice(&body[2..34]);
    let mut cursor = 34;
    let session_len = *body.get(cursor).ok_or(ClientHelloError::Malformed)? as usize;
    cursor += 1;
    cursor = cursor
        .checked_add(session_len)
        .ok_or(ClientHelloError::Malformed)?;
    let cipher_suite = u16::from_be_bytes(
        body.get(cursor..cursor + 2)
            .ok_or(ClientHelloError::Truncated)?
            .try_into()
            .expect("fixed slice"),
    );
    cursor += 2;
    cursor += 1;
    let extensions_len = read_u16(body, &mut cursor)?;
    let extensions_end = cursor
        .checked_add(extensions_len)
        .ok_or(ClientHelloError::Malformed)?;
    if extensions_end > body.len() {
        return Err(ClientHelloError::Truncated);
    }
    let mut x25519_key_share = None;
    while cursor < extensions_end {
        let extension_type = read_u16(body, &mut cursor)?;
        let extension_len = read_u16(body, &mut cursor)?;
        let extension_end = cursor
            .checked_add(extension_len)
            .ok_or(ClientHelloError::Malformed)?;
        if extension_end > extensions_end {
            return Err(ClientHelloError::Truncated);
        }
        let extension = &body[cursor..extension_end];
        if extension_type == 0x0033 && extension.len() >= 4 {
            let group = u16::from_be_bytes([extension[0], extension[1]]);
            let key_len = u16::from_be_bytes([extension[2], extension[3]]) as usize;
            if group == 0x001d && key_len == 32 && extension.len() == 4 + key_len {
                x25519_key_share = Some(extension[4..].to_vec());
            }
        }
        cursor = extension_end;
    }
    Ok(ServerHelloInfo {
        record,
        handshake,
        random,
        cipher_suite,
        x25519_key_share,
    })
}

fn collect_handshake(
    input: &[u8],
    expected_type: u8,
) -> Result<(Vec<u8>, Vec<u8>), ClientHelloError> {
    if input.len() < 5 {
        return Err(ClientHelloError::Truncated);
    }
    let mut record_end = 0usize;
    let mut handshake = Vec::new();
    loop {
        if input.len() < record_end + 5 {
            return Err(ClientHelloError::Truncated);
        }
        let record_header = &input[record_end..record_end + 5];
        if record_header[0] != 0x16 || record_header[1] != 0x03 {
            return Err(ClientHelloError::UnexpectedType);
        }
        let record_len = u16::from_be_bytes([record_header[3], record_header[4]]) as usize;
        let payload_start = record_end + 5;
        let next_record_end = payload_start
            .checked_add(record_len)
            .ok_or(ClientHelloError::Malformed)?;
        if input.len() < next_record_end {
            return Err(ClientHelloError::Truncated);
        }
        handshake.extend_from_slice(&input[payload_start..next_record_end]);
        record_end = next_record_end;
        if handshake.len() < 4 {
            continue;
        }
        if handshake[0] != expected_type {
            return Err(ClientHelloError::UnexpectedType);
        }
        let message_len =
            u32::from_be_bytes([0, handshake[1], handshake[2], handshake[3]]) as usize;
        if message_len + 4 > handshake.len() {
            continue;
        }
        return Ok((
            input[..record_end].to_vec(),
            handshake[..4 + message_len].to_vec(),
        ));
    }
}

fn read_u16(input: &[u8], cursor: &mut usize) -> Result<usize, ClientHelloError> {
    let bytes = input
        .get(*cursor..*cursor + 2)
        .ok_or(ClientHelloError::Truncated)?;
    *cursor += 2;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]) as usize)
}

fn parse_server_name(extension: &[u8]) -> Option<String> {
    if extension.len() < 5 {
        return None;
    }
    let list_len = u16::from_be_bytes([extension[0], extension[1]]) as usize;
    if list_len + 2 > extension.len() || extension[2] != 0 {
        return None;
    }
    let name_len = u16::from_be_bytes([extension[3], extension[4]]) as usize;
    std::str::from_utf8(extension.get(5..5 + name_len)?)
        .ok()
        .map(str::to_owned)
}

fn parse_signature_schemes(extension: &[u8]) -> Option<Vec<u16>> {
    if extension.len() < 2 {
        return None;
    }
    let length = u16::from_be_bytes([extension[0], extension[1]]) as usize;
    if length != extension.len().saturating_sub(2) || !length.is_multiple_of(2) {
        return None;
    }
    Some(
        extension[2..]
            .chunks_exact(2)
            .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
            .collect(),
    )
}

fn parse_alpn(extension: &[u8]) -> Vec<String> {
    if extension.len() < 2 {
        return Vec::new();
    }
    let length = u16::from_be_bytes([extension[0], extension[1]]) as usize;
    if length != extension.len().saturating_sub(2) {
        return Vec::new();
    }
    let mut cursor = 2;
    let mut protocols = Vec::new();
    while cursor < extension.len() {
        let Some(&length) = extension.get(cursor) else {
            return Vec::new();
        };
        let length = length as usize;
        cursor += 1;
        let Some(protocol) = extension.get(cursor..cursor + length) else {
            return Vec::new();
        };
        protocols.push(String::from_utf8_lossy(protocol).into_owned());
        cursor += length;
    }
    protocols
}

fn parse_x25519_key_share(extension: &[u8]) -> Option<Vec<u8>> {
    if extension.len() < 2 {
        return None;
    }
    let list_len = u16::from_be_bytes([extension[0], extension[1]]) as usize;
    let list_end = list_len.checked_add(2)?;
    if list_end > extension.len() {
        return None;
    }
    let mut cursor = 2;
    while cursor < list_end {
        let group = u16::from_be_bytes(extension.get(cursor..cursor + 2)?.try_into().ok()?);
        let key_len =
            u16::from_be_bytes(extension.get(cursor + 2..cursor + 4)?.try_into().ok()?) as usize;
        cursor += 4;
        let key_end = cursor.checked_add(key_len)?;
        if key_end > list_end {
            return None;
        }
        if group == 0x001d && key_len == 32 {
            return Some(extension[cursor..key_end].to_vec());
        }
        cursor = key_end;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client_hello() -> Vec<u8> {
        let mut body = vec![0x03, 0x03];
        body.extend_from_slice(&[7u8; 32]);
        body.extend_from_slice(&[4, 1, 2, 3, 4]);
        body.extend_from_slice(&[0, 2, 0x13, 0x01]);
        body.extend_from_slice(&[1, 0]);
        let name = b"xhttp.example";
        let mut sni = vec![0, (name.len() + 3) as u8, 0, 0, name.len() as u8];
        sni.extend_from_slice(name);
        let mut key_share = vec![0, 36, 0x00, 0x1d, 0, 32];
        key_share.extend_from_slice(&[9u8; 32]);
        let mut extensions = vec![0, 0, 0, sni.len() as u8];
        extensions.extend_from_slice(&sni);
        extensions.extend_from_slice(&[0, 0x33, 0, key_share.len() as u8]);
        extensions.extend_from_slice(&key_share);
        body.push((extensions.len() >> 8) as u8);
        body.push(extensions.len() as u8);
        body.extend_from_slice(&extensions);
        let mut handshake = vec![
            1,
            (body.len() >> 16) as u8,
            (body.len() >> 8) as u8,
            body.len() as u8,
        ];
        handshake.extend_from_slice(&body);
        let mut record = vec![
            0x16,
            0x03,
            0x03,
            (handshake.len() >> 8) as u8,
            handshake.len() as u8,
        ];
        record.extend_from_slice(&handshake);
        record
    }

    #[test]
    fn parses_sni_random_session_and_x25519_share() {
        let info = parse_client_hello(&client_hello()).unwrap();
        assert_eq!(info.random, [7u8; 32]);
        assert_eq!(info.session_id, vec![1, 2, 3, 4]);
        assert_eq!(info.server_name.as_deref(), Some("xhttp.example"));
        assert_eq!(info.x25519_key_share, Some(vec![9u8; 32]));
    }

    #[test]
    fn parses_client_hello_split_across_tls_records() {
        let single_record = client_hello();
        let payload = &single_record[5..];
        let split = 19;
        let mut fragmented = Vec::with_capacity(single_record.len() + 5);
        for chunk in [&payload[..split], &payload[split..]] {
            fragmented.extend_from_slice(&[0x16, 0x03, 0x03]);
            fragmented.extend_from_slice(&(chunk.len() as u16).to_be_bytes());
            fragmented.extend_from_slice(chunk);
        }
        let info = parse_client_hello(&fragmented).unwrap();
        assert_eq!(info.record.len(), fragmented.len());
        assert_eq!(info.random, [7u8; 32]);
        assert_eq!(info.server_name.as_deref(), Some("xhttp.example"));
    }

    #[test]
    fn parses_server_hello_cipher_and_x25519_key_share() {
        let mut body = vec![0x03, 0x03];
        body.extend_from_slice(&[8u8; 32]);
        body.push(4);
        body.extend_from_slice(&[1, 2, 3, 4]);
        body.extend_from_slice(&[0x13, 0x01, 0]);
        let mut extension = vec![0, 0x1d, 0, 32];
        extension.extend_from_slice(&[6u8; 32]);
        let extension_record = [0, 0x33, 0, extension.len() as u8]
            .into_iter()
            .chain(extension)
            .collect::<Vec<_>>();
        body.extend_from_slice(&(extension_record.len() as u16).to_be_bytes());
        body.extend_from_slice(&extension_record);
        let mut handshake = vec![2, 0, (body.len() >> 8) as u8, body.len() as u8];
        handshake.extend_from_slice(&body);
        let mut record = vec![
            0x16,
            0x03,
            0x03,
            (handshake.len() >> 8) as u8,
            handshake.len() as u8,
        ];
        record.extend_from_slice(&handshake);

        let info = parse_server_hello(&record).unwrap();
        assert_eq!(info.random, [8u8; 32]);
        assert_eq!(info.cipher_suite, 0x1301);
        assert_eq!(info.x25519_key_share, Some(vec![6u8; 32]));
    }
}

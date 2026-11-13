#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientHelloInfo {
    pub record: Vec<u8>,
    pub random: [u8; 32],
    pub session_id: Vec<u8>,
    pub server_name: Option<String>,
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
    if input[0] != 0x16 || input[1] != 0x03 {
        return Err(ClientHelloError::UnexpectedType);
    }
    let record_len = u16::from_be_bytes([input[3], input[4]]) as usize;
    if input.len() < 5 + record_len {
        return Err(ClientHelloError::Truncated);
    }
    let record = &input[..5 + record_len];
    let payload = &record[5..];
    if payload.len() < 4 || payload[0] != 1 {
        return Err(ClientHelloError::UnexpectedType);
    }
    let hello_len = u32::from_be_bytes([0, payload[1], payload[2], payload[3]]) as usize;
    if hello_len + 4 > payload.len() {
        return Err(ClientHelloError::Truncated);
    }
    let hello = &payload[4..4 + hello_len];
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
    cursor = cursor
        .checked_add(cipher_len)
        .ok_or(ClientHelloError::Malformed)?;
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
            0x0033 => x25519_key_share = parse_x25519_key_share(extension),
            _ => {}
        }
        cursor = extension_end;
    }
    Ok(ClientHelloInfo {
        record: record.to_vec(),
        random,
        session_id,
        server_name,
        x25519_key_share,
    })
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
}

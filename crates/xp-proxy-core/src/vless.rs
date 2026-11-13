use bytes::Bytes;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VlessRequest {
    pub user: Uuid,
    pub command: u8,
    pub destination: (String, u16),
    pub addons: Bytes,
}

pub fn is_authorized(request: &VlessRequest, users: &[Uuid]) -> bool {
    users.iter().any(|user| user == &request.user)
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum VlessRequestError {
    #[error("request is truncated")]
    Truncated,
    #[error("unsupported VLESS version {0}")]
    UnsupportedVersion(u8),
    #[error("invalid UUID")]
    InvalidUuid,
    #[error("unsupported command {0}")]
    UnsupportedCommand(u8),
    #[error("invalid address")]
    InvalidAddress,
}

pub fn decode_request(input: &[u8]) -> Result<(VlessRequest, usize), VlessRequestError> {
    if input.len() < 1 + 16 + 1 + 1 + 2 + 1 {
        return Err(VlessRequestError::Truncated);
    }
    if input[0] != 1 {
        return Err(VlessRequestError::UnsupportedVersion(input[0]));
    }
    let user = Uuid::from_slice(&input[1..17]).map_err(|_| VlessRequestError::InvalidUuid)?;
    let addon_len = input[17] as usize;
    let addon_end = 18usize
        .checked_add(addon_len)
        .ok_or(VlessRequestError::Truncated)?;
    if input.len() < addon_end + 4 {
        return Err(VlessRequestError::Truncated);
    }
    let command = input[addon_end];
    if command != 1 {
        return Err(VlessRequestError::UnsupportedCommand(command));
    }
    if input.len() < addon_end + 4 {
        return Err(VlessRequestError::Truncated);
    }
    let port = u16::from_be_bytes([input[addon_end + 1], input[addon_end + 2]]);
    let address_type = input[addon_end + 3];
    let mut cursor = addon_end + 4;
    let address = match address_type {
        1 => {
            if input.len() < cursor + 4 {
                return Err(VlessRequestError::Truncated);
            }
            let ip = Ipv4Addr::new(
                input[cursor],
                input[cursor + 1],
                input[cursor + 2],
                input[cursor + 3],
            );
            cursor += 4;
            IpAddr::V4(ip).to_string()
        }
        2 => {
            if input.len() < cursor + 1 {
                return Err(VlessRequestError::Truncated);
            }
            let len = input[cursor] as usize;
            cursor += 1;
            if input.len() < cursor + len {
                return Err(VlessRequestError::Truncated);
            }
            let host = std::str::from_utf8(&input[cursor..cursor + len])
                .map_err(|_| VlessRequestError::InvalidAddress)?
                .to_string();
            cursor += len;
            host
        }
        3 => {
            if input.len() < cursor + 16 {
                return Err(VlessRequestError::Truncated);
            }
            let mut octets = [0u8; 16];
            octets.copy_from_slice(&input[cursor..cursor + 16]);
            cursor += 16;
            IpAddr::V6(Ipv6Addr::from(octets)).to_string()
        }
        _ => return Err(VlessRequestError::InvalidAddress),
    };
    Ok((
        VlessRequest {
            user,
            command,
            destination: (address, port),
            addons: Bytes::copy_from_slice(&input[18..addon_end]),
        },
        cursor,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_domain_request() {
        let uuid = Uuid::nil();
        let mut input = vec![1];
        input.extend_from_slice(uuid.as_bytes());
        input.extend_from_slice(&[0, 1, 0x01, 0xbb, 2, 11]);
        input.extend_from_slice(b"example.com");
        let (request, used) = decode_request(&input).unwrap();
        assert_eq!(request.destination, ("example.com".into(), 443));
        assert_eq!(used, input.len());
    }
}

use crate::domain::{PortOccupancy, ProcessSummary};
use crate::process_manager::{find_process_summary, protection};
use netstat2::{get_sockets_info, AddressFamilyFlags, ProtocolFlags, ProtocolSocketInfo};

fn associated_pids_or_unknown(pids: &[u32]) -> Vec<u32> {
    if pids.is_empty() {
        return vec![0];
    }
    let mut unique = pids.to_vec();
    unique.sort_unstable();
    unique.dedup();
    unique
}

fn normalize_protocol(protocol: &str) -> Result<String, String> {
    let proto = protocol.trim().to_lowercase();
    match proto.as_str() {
        "tcp" | "udp" | "both" | "all" | "" => Ok(proto),
        _ => Err("PORT_INVALID:仅支持 tcp、udp 或 both".to_string()),
    }
}

pub fn inspect_port(protocol: &str, port: u16) -> Result<Vec<PortOccupancy>, String> {
    if port == 0 {
        return Err("PORT_INVALID:端口无效".to_string());
    }
    let proto = normalize_protocol(protocol)?;
    let af = AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6;
    let pf = match proto.as_str() {
        "tcp" => ProtocolFlags::TCP,
        "udp" => ProtocolFlags::UDP,
        "both" | "all" | "" => ProtocolFlags::TCP | ProtocolFlags::UDP,
        _ => unreachable!("normalize_protocol only returns supported protocols"),
    };
    let filter_tcp = proto == "tcp" || proto == "both" || proto == "all" || proto.is_empty();
    let filter_udp = proto == "udp" || proto == "both" || proto == "all" || proto.is_empty();

    let sockets = get_sockets_info(af, pf).map_err(|e| format!("PORT_QUERY_FAILED:{}", e))?;

    let mut result = Vec::new();
    for sock in sockets {
        let (local_port, listen_addr, sock_proto) = match &sock.protocol_socket_info {
            ProtocolSocketInfo::Tcp(t) => (t.local_port, t.local_addr.to_string(), "tcp"),
            ProtocolSocketInfo::Udp(u) => (u.local_port, u.local_addr.to_string(), "udp"),
        };
        if local_port != port {
            continue;
        }
        if sock_proto == "tcp" && !filter_tcp {
            continue;
        }
        if sock_proto == "udp" && !filter_udp {
            continue;
        }

        for pid in associated_pids_or_unknown(&sock.associated_pids) {
            let process = if pid > 0 {
                find_process_summary(pid).unwrap_or(ProcessSummary {
                    pid,
                    name: "unknown".to_string(),
                    working_directory: None,
                    command_line: None,
                })
            } else {
                ProcessSummary {
                    pid: 0,
                    name: "unknown".to_string(),
                    working_directory: None,
                    command_line: None,
                }
            };
            let prot = protection(&process);
            result.push(PortOccupancy {
                protocol: sock_proto.to_string(),
                port,
                listen_address: listen_addr.clone(),
                process,
                protection: prot,
            });
        }
    }
    Ok(result)
}

pub fn ports_for_pid(pid: u32) -> Vec<u16> {
    let af = AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6;
    let pf = ProtocolFlags::TCP | ProtocolFlags::UDP;
    let sockets = get_sockets_info(af, pf).unwrap_or_default();
    let mut ports = Vec::new();
    for sock in sockets {
        if !sock.associated_pids.contains(&pid) {
            continue;
        }
        let port = match &sock.protocol_socket_info {
            ProtocolSocketInfo::Tcp(t) => t.local_port,
            ProtocolSocketInfo::Udp(u) => u.local_port,
        };
        if port > 0 {
            ports.push(port);
        }
    }
    ports.sort_unstable();
    ports.dedup();
    ports
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn associated_pids_are_deduplicated_and_sorted() {
        assert_eq!(associated_pids_or_unknown(&[9, 3, 9, 5]), vec![3, 5, 9]);
    }

    #[test]
    fn empty_associated_pids_keep_unknown_row() {
        assert_eq!(associated_pids_or_unknown(&[]), vec![0]);
    }

    #[test]
    fn protocol_is_trimmed_and_normalized() {
        assert_eq!(normalize_protocol(" TCP ").unwrap(), "tcp");
        assert_eq!(normalize_protocol("Udp").unwrap(), "udp");
        assert_eq!(normalize_protocol(" BOTH ").unwrap(), "both");
    }

    #[test]
    fn unknown_protocol_is_rejected() {
        assert!(normalize_protocol("http").is_err());
    }
}
